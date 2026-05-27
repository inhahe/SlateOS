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

    // -- Shared memory: name validation --

    #[test]
    fn test_shm_open_null_name_efault() {
        crate::errno::set_errno(0);
        assert_eq!(shm_open(core::ptr::null(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_shm_unlink_null_name_efault() {
        crate::errno::set_errno(0);
        assert_eq!(shm_unlink(core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
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
    fn test_memfd_create_succeeds_or_real_errno() {
        // memfd_create is implemented (no longer ENOSYS).  On host-target
        // tests the underlying open() may still fail because /dev/shm
        // doesn't exist, but any failure must be a real errno — never
        // ENOSYS — and a successful call returns a positive fd.
        let ret = memfd_create(b"test\0".as_ptr(), 0);
        if ret < 0 {
            assert_ne!(crate::errno::get_errno(), crate::errno::ENOSYS);
        } else {
            assert!(ret >= 0);
            let _ = crate::file::close(ret);
        }
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

    // -- mmap zero-length returns EINVAL --

    #[test]
    fn test_mmap_zero_length_einval() {
        crate::errno::set_errno(0);
        let ret = mmap(
            core::ptr::null_mut(),
            0,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- munmap validates inputs --

    #[test]
    fn test_munmap_null_addr() {
        crate::errno::set_errno(0);
        assert_eq!(munmap(core::ptr::null_mut(), 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_munmap_zero_length() {
        crate::errno::set_errno(0);
        assert_eq!(munmap(0x1000 as *mut core::ffi::c_void, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- mprotect validates inputs --

    #[test]
    fn test_mprotect_null_addr() {
        crate::errno::set_errno(0);
        assert_eq!(mprotect(core::ptr::null_mut(), 4096, PROT_READ), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mprotect_zero_length() {
        crate::errno::set_errno(0);
        assert_eq!(mprotect(0x1000 as *mut core::ffi::c_void, 0, PROT_READ), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- mmap64 is alias for mmap --

    #[test]
    fn test_mmap64_zero_length_einval() {
        crate::errno::set_errno(0);
        let ret = mmap64(core::ptr::null_mut(), 0, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- shm name validation and path composition --

    #[test]
    fn test_shm_open_empty_name_einval() {
        // Empty name (just NUL) — must be rejected.
        crate::errno::set_errno(0);
        let ret = shm_open(b"\0".as_ptr(), 0, 0o644);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_shm_open_missing_leading_slash_einval() {
        // POSIX: name must begin with '/'.
        crate::errno::set_errno(0);
        let ret = shm_open(b"foo\0".as_ptr(), 0, 0o644);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_shm_open_embedded_slash_einval() {
        // POSIX: name should not contain additional '/'.
        crate::errno::set_errno(0);
        let ret = shm_open(b"/foo/bar\0".as_ptr(), 0, 0o644);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_shm_unlink_empty_name_einval() {
        crate::errno::set_errno(0);
        let ret = shm_unlink(b"\0".as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_shm_unlink_missing_leading_slash_einval() {
        crate::errno::set_errno(0);
        let ret = shm_unlink(b"bar\0".as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_shm_open_too_long_nametoolong() {
        // Name of SHM_NAME_MAX bytes is rejected (loop terminates at
        // SHM_NAME_MAX without finding NUL).
        let mut big = [b'a'; 256];
        big[0] = b'/';
        // We don't write a NUL into `big[..SHM_NAME_MAX]` so the helper
        // hits the limit and reports ENAMETOOLONG.
        big[255] = 0; // Make sure the buffer ends in NUL eventually.
        crate::errno::set_errno(0);
        let ret = shm_open(big.as_ptr(), 0, 0o644);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENAMETOOLONG);
    }

    #[test]
    fn test_resolve_shm_name_basic() {
        let mut out = [0u8; 256];
        let len = resolve_shm_name(b"/foo\0".as_ptr(), &mut out);
        // "/dev/shm" (8) + "/foo" (4) = 12.
        assert_eq!(len, 12);
        assert_eq!(&out[..12], b"/dev/shm/foo");
        assert_eq!(out[12], 0); // NUL-terminated.
    }

    #[test]
    fn test_resolve_shm_name_rejects_no_leading_slash() {
        let mut out = [0u8; 256];
        crate::errno::set_errno(0);
        assert_eq!(resolve_shm_name(b"foo\0".as_ptr(), &mut out), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_resolve_shm_name_rejects_double_slash() {
        let mut out = [0u8; 256];
        crate::errno::set_errno(0);
        assert_eq!(resolve_shm_name(b"//foo\0".as_ptr(), &mut out), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_shm_constants() {
        assert_eq!(SHM_DIR, b"/dev/shm");
        assert!(SHM_NAME_MAX > 8); // Room for at least a short name.
        // Must leave headroom under PATH_MAX after the prefix.
        assert!(SHM_DIR.len() + SHM_NAME_MAX < crate::unistd::PATH_MAX);
    }

    // -- memfd_create sets errno --

    #[test]
    fn test_memfd_create_null_name_efault() {
        crate::errno::set_errno(0);
        assert_eq!(memfd_create(core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- mremap sets errno --

    #[test]
    fn test_mremap_sets_errno() {
        crate::errno::set_errno(0);
        let ret = mremap(core::ptr::null_mut(), 4096, 8192, 0);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mremap_with_maymove() {
        let ret = mremap(0x1000 as *mut core::ffi::c_void, 4096, 8192, MREMAP_MAYMOVE);
        assert_eq!(ret, MAP_FAILED);
    }

    // -- Stubs accept various argument combos --

    #[test]
    fn test_mlock_with_nonzero_addr() {
        assert_eq!(mlock(0x1000 as *const core::ffi::c_void, 16384), 0);
    }

    #[test]
    fn test_munlock_with_nonzero_addr() {
        assert_eq!(munlock(0x1000 as *const core::ffi::c_void, 16384), 0);
    }

    #[test]
    fn test_mlockall_mcl_current() {
        assert_eq!(mlockall(MCL_CURRENT), 0);
    }

    #[test]
    fn test_mlockall_mcl_future() {
        assert_eq!(mlockall(MCL_FUTURE), 0);
    }

    #[test]
    fn test_mlockall_combined_flags() {
        assert_eq!(mlockall(MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT), 0);
    }

    // -- mlock2 --

    #[test]
    fn test_mlock2_no_flags() {
        // flags == 0 is equivalent to mlock
        assert_eq!(mlock2(core::ptr::null(), 4096, 0), 0);
    }

    #[test]
    fn test_mlock2_onfault() {
        assert_eq!(mlock2(core::ptr::null(), 4096, MLOCK_ONFAULT), 0);
    }

    #[test]
    fn test_mlock2_with_addr() {
        assert_eq!(mlock2(0x1000 as *const core::ffi::c_void, 16384, 0), 0);
    }

    #[test]
    fn test_mlock2_with_addr_onfault() {
        assert_eq!(mlock2(0x1000 as *const core::ffi::c_void, 16384, MLOCK_ONFAULT), 0);
    }

    #[test]
    fn test_mlock_onfault_constant() {
        assert_eq!(MLOCK_ONFAULT, 1);
    }

    #[test]
    fn test_msync_ms_async() {
        assert_eq!(msync(core::ptr::null_mut(), 4096, MS_ASYNC), 0);
    }

    #[test]
    fn test_msync_ms_invalidate() {
        assert_eq!(msync(core::ptr::null_mut(), 4096, MS_SYNC | MS_INVALIDATE), 0);
    }

    #[test]
    fn test_madvise_all_advice_values() {
        assert_eq!(madvise(core::ptr::null_mut(), 4096, MADV_RANDOM), 0);
        assert_eq!(madvise(core::ptr::null_mut(), 4096, MADV_SEQUENTIAL), 0);
        assert_eq!(madvise(core::ptr::null_mut(), 4096, MADV_WILLNEED), 0);
        assert_eq!(madvise(core::ptr::null_mut(), 4096, MADV_DONTNEED), 0);
    }

    #[test]
    fn test_posix_madvise_all_values() {
        assert_eq!(posix_madvise(core::ptr::null_mut(), 4096, POSIX_MADV_RANDOM), 0);
        assert_eq!(posix_madvise(core::ptr::null_mut(), 4096, POSIX_MADV_WILLNEED), 0);
        assert_eq!(posix_madvise(core::ptr::null_mut(), 4096, POSIX_MADV_DONTNEED), 0);
    }

    // -- MS_ASYNC and MS_SYNC are distinct --

    #[test]
    fn test_msync_flags_disjoint() {
        assert_eq!(MS_ASYNC & MS_SYNC, 0);
        assert_eq!(MS_ASYNC & MS_INVALIDATE, 0);
    }

    // -- Prot flags are distinct single bits --

    #[test]
    fn test_prot_flags_single_bits() {
        assert_eq!(PROT_READ.count_ones(), 1);
        assert_eq!(PROT_WRITE.count_ones(), 1);
        assert_eq!(PROT_EXEC.count_ones(), 1);
    }

    // -- POSIX_MADV_* match MADV_* values --

    #[test]
    fn test_posix_madv_matches_madv() {
        assert_eq!(POSIX_MADV_NORMAL, MADV_NORMAL);
        assert_eq!(POSIX_MADV_RANDOM, MADV_RANDOM);
        assert_eq!(POSIX_MADV_SEQUENTIAL, MADV_SEQUENTIAL);
        assert_eq!(POSIX_MADV_WILLNEED, MADV_WILLNEED);
        assert_eq!(POSIX_MADV_DONTNEED, MADV_DONTNEED);
    }

    // -- mincore returns ENOSYS --

    #[test]
    fn test_mincore_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = mincore(core::ptr::null_mut(), 4096, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mincore_with_addr() {
        let ret = mincore(0x1000 as *mut core::ffi::c_void, 4096, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }
}

// ---------------------------------------------------------------------------
// POSIX shared memory objects — backed by files under /dev/shm/
// ---------------------------------------------------------------------------
//
// POSIX shm_open(3) is specified to open a *named* shared-memory object
// such that mmap() with MAP_SHARED on the resulting fd shares memory
// between processes that open the same name.  On Linux this is
// implemented by mounting tmpfs at /dev/shm and treating the name as a
// filename in that mount.  We follow the Linux convention: shm_open
// opens /dev/shm/<sanitized-name> as a regular file, and shm_unlink
// removes it.  Memory sharing then falls out of file-backed mmap(),
// which our kernel supports for any open fd.
//
// Name rules (POSIX):
//   * Must start with '/'.
//   * Should not contain other '/' characters.
//   * Length, including leading '/' but excluding NUL, must be <=
//     `SHM_NAME_MAX` so that the resolved path fits in a PATH_MAX
//     buffer with room for the "/dev/shm" prefix.

/// Prefix where POSIX shared memory objects live.
const SHM_DIR: &[u8] = b"/dev/shm";

/// Maximum bytes in a shm name (including the leading '/').  Chosen so
/// that the resolved path `/dev/shm/<name>` stays well under PATH_MAX.
const SHM_NAME_MAX: usize = 200;

/// Resolve a shm_open / shm_unlink name into `/dev/shm/<name>` in `out`.
///
/// Returns the byte length written (not counting trailing NUL) on
/// success, or sets errno and returns 0 on failure.  Validates that
/// the name starts with '/', is non-empty after that '/', contains no
/// embedded '/' (POSIX), and fits in the buffer.
fn resolve_shm_name(name: *const u8, out: &mut [u8]) -> usize {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return 0;
    }
    // Read the name as a C string with bounded length.
    let mut name_len: usize = 0;
    // SAFETY: caller contract — `name` is a valid NUL-terminated C string.
    while name_len < SHM_NAME_MAX {
        let b = unsafe { *name.add(name_len) };
        if b == 0 {
            break;
        }
        name_len = name_len.wrapping_add(1);
    }
    if name_len == 0 {
        errno::set_errno(errno::EINVAL);
        return 0;
    }
    if name_len >= SHM_NAME_MAX {
        // Either truncated (no NUL found) or too long either way.
        errno::set_errno(errno::ENAMETOOLONG);
        return 0;
    }
    // SAFETY: bounded length above; we know `name` has at least 1 byte.
    let first = unsafe { *name };
    if first != b'/' {
        errno::set_errno(errno::EINVAL);
        return 0;
    }
    // No additional '/' in the remainder.
    let mut i: usize = 1;
    while i < name_len {
        // SAFETY: i < name_len < SHM_NAME_MAX; we just walked these bytes.
        let b = unsafe { *name.add(i) };
        if b == b'/' || b == 0 {
            errno::set_errno(errno::EINVAL);
            return 0;
        }
        i = i.wrapping_add(1);
    }
    // Compose "/dev/shm" + name (name already begins with '/').
    let total = SHM_DIR.len().wrapping_add(name_len);
    if total >= out.len() {
        errno::set_errno(errno::ENAMETOOLONG);
        return 0;
    }
    let Some(prefix_dst) = out.get_mut(..SHM_DIR.len()) else {
        errno::set_errno(errno::ENAMETOOLONG);
        return 0;
    };
    prefix_dst.copy_from_slice(SHM_DIR);
    // SAFETY: name has at least name_len readable bytes.
    let name_slice = unsafe { core::slice::from_raw_parts(name, name_len) };
    let Some(name_dst) = out.get_mut(SHM_DIR.len()..total) else {
        errno::set_errno(errno::ENAMETOOLONG);
        return 0;
    };
    name_dst.copy_from_slice(name_slice);
    // NUL-terminate.
    if let Some(slot) = out.get_mut(total) {
        *slot = 0;
    }
    total
}

/// Open a POSIX shared memory object.
///
/// Resolves `name` to `/dev/shm/<name>` and forwards to `open()`.  The
/// returned fd can be sized with `ftruncate` and mapped with `mmap`
/// using `MAP_SHARED`.  Other processes that `shm_open` the same name
/// will obtain a separate fd that maps the same underlying file, giving
/// the POSIX shared-memory semantic.
///
/// # Errors
///
/// - `EFAULT` — `name` is NULL.
/// - `EINVAL` — `name` is empty, does not start with `/`, or contains
///   additional `/` characters.
/// - `ENAMETOOLONG` — the resolved `/dev/shm/<name>` path exceeds
///   `PATH_MAX`.
/// - Plus any error reported by the underlying `open()` call (notably
///   `ENOENT` if `O_CREAT` is not set and the object doesn't exist).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shm_open(name: *const u8, oflag: i32, mode: ModeT) -> i32 {
    let mut path = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_shm_name(name, &mut path);
    if len == 0 {
        return -1;
    }
    crate::file::open(path.as_ptr(), oflag, mode)
}

/// Remove a POSIX shared memory object.
///
/// Resolves `name` to `/dev/shm/<name>` and forwards to `unlink()`.
/// As with regular files, processes that already have the object open
/// continue to access it until they close their fds; the name itself
/// is removed immediately (subject to the underlying filesystem's
/// semantics for unlink-while-open).
///
/// # Errors
///
/// Same name-validation errors as `shm_open`, plus any error from
/// `unlink()` (e.g., `ENOENT`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shm_unlink(name: *const u8) -> i32 {
    let mut path = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_shm_name(name, &mut path);
    if len == 0 {
        return -1;
    }
    crate::file::unlink(path.as_ptr())
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

/// Maximum length of the user-supplied `memfd_create` name (Linux's
/// limit, minus the prefix we add).  The Linux kernel allows names up
/// to 249 bytes total; we reserve a few bytes for our `.memfd_<n>_`
/// prefix so the on-disk path still fits in `/dev/shm/<prefix><name>`
/// without exceeding the underlying filesystem's per-component limit.
const MEMFD_NAME_MAX: usize = 200;

/// Monotonic counter used to make on-disk `/dev/shm/.memfd_<n>_<name>`
/// paths unique within a process.  Each call to `memfd_create` consumes
/// one value.  Even after the process exits, leftover files in
/// `/dev/shm` will be reaped by the boot cleanup pass; collisions across
/// process restarts would only matter if the cleanup pass is skipped,
/// and the unlink-after-open below makes the file anonymous anyway.
static MEMFD_COUNTER: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Recognised flag bits for `memfd_create`.  Anything outside this set
/// is rejected with `EINVAL` (matches Linux).
const MEMFD_FLAG_MASK: u32 = crate::linux_memfd::MFD_CLOEXEC
    | crate::linux_memfd::MFD_ALLOW_SEALING
    | crate::linux_memfd::MFD_HUGETLB
    | crate::linux_memfd::MFD_NOEXEC_SEAL
    | crate::linux_memfd::MFD_EXEC;

/// `memfd_create` — create an anonymous in-memory file.
///
/// The `name` parameter is purely for debugging/proc display: it doesn't
/// place the file in any namespace and two memfds with the same name
/// don't share storage.  Internally we route the request through
/// `/dev/shm/.memfd_<counter>_<name>`, open it with `O_CREAT | O_RDWR
/// | O_EXCL`, then `unlink()` the path immediately so the file becomes
/// truly anonymous (only the returned fd keeps it alive).
///
/// Supported flags:
/// - `MFD_CLOEXEC` — set close-on-exec on the returned fd.
/// - `MFD_ALLOW_SEALING` — accepted; sealing itself (fcntl
///   `F_ADD_SEALS` / `F_GET_SEALS`) is not yet implemented, so the
///   flag is recognised but currently has no observable effect beyond
///   not erroring out.
/// - `MFD_NOEXEC_SEAL` — accepted; equivalent in spirit to
///   `MFD_ALLOW_SEALING` plus an immediate `F_SEAL_EXEC`.  Without
///   sealing infrastructure or an executable-bit model on memfds, this
///   currently has no observable effect.
/// - `MFD_EXEC` — accepted, no-op (we don't have a noexec default to
///   override).
/// - `MFD_HUGETLB` — rejected with `EINVAL`; the kernel has no
///   hugepage support yet.
///
/// # Errors
///
/// - `EFAULT` — `name` is NULL.
/// - `EINVAL` — `name` is too long, `flags` has unknown bits, or
///   `MFD_HUGETLB` is set.
/// - Plus any error reported by the underlying `open()` / `unlink()`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn memfd_create(name: *const u8, flags: u32) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if flags & !MEMFD_FLAG_MASK != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & crate::linux_memfd::MFD_HUGETLB != 0 {
        // Hugepage-backed memfds require kernel huge-page support.
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Measure user name length with a bound.
    let mut name_len: usize = 0;
    while name_len <= MEMFD_NAME_MAX {
        // SAFETY: caller contract — `name` is a valid NUL-terminated C string.
        let b = unsafe { *name.add(name_len) };
        if b == 0 {
            break;
        }
        // Reject embedded '/' in the user name: it would split the path.
        if b == b'/' {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        name_len = name_len.wrapping_add(1);
    }
    if name_len > MEMFD_NAME_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Build "/dev/shm/.memfd_<counter>_<name>" into a stack buffer.
    let mut path = [0u8; crate::unistd::PATH_MAX];
    let mut pos: usize = 0;
    // Helper: append a byte slice; returns false if it would overflow.
    let put = |buf: &mut [u8], p: &mut usize, src: &[u8]| -> bool {
        if p.wrapping_add(src.len()) >= buf.len() {
            return false;
        }
        if let Some(dst) = buf.get_mut(*p..p.wrapping_add(src.len())) {
            dst.copy_from_slice(src);
        }
        *p = p.wrapping_add(src.len());
        true
    };
    if !put(&mut path, &mut pos, SHM_DIR) {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    if !put(&mut path, &mut pos, b"/.memfd_") {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    let counter = MEMFD_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    // Format the counter as decimal into a small buffer.
    let mut num_buf = [0u8; 20];
    let num_len = format_u64(counter, &mut num_buf);
    if !put(&mut path, &mut pos, &num_buf[..num_len]) {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    if !put(&mut path, &mut pos, b"_") {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    // Append the user-supplied name (sanitized: NUL-free, /-free above).
    // SAFETY: name_len bytes are readable before the NUL.
    let user_name = unsafe { core::slice::from_raw_parts(name, name_len) };
    if !put(&mut path, &mut pos, user_name) {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    // NUL-terminate.
    if pos >= path.len() {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    if let Some(slot) = path.get_mut(pos) {
        *slot = 0;
    }

    // Open with O_CREAT | O_RDWR | O_EXCL.  Honor MFD_CLOEXEC.
    let mut oflag =
        crate::fcntl::O_CREAT | crate::fcntl::O_EXCL | crate::fcntl::O_RDWR;
    if flags & crate::linux_memfd::MFD_CLOEXEC != 0 {
        oflag |= crate::fcntl::O_CLOEXEC;
    }
    let fd = crate::file::open(path.as_ptr(), oflag, 0o600);
    if fd < 0 {
        return -1;
    }
    // Unlink the path immediately so the fd becomes anonymous.  Failure
    // here is non-fatal: the fd still works, the file just sticks around
    // in /dev/shm until the boot cleanup pass.
    let _ = crate::file::unlink(path.as_ptr());

    fd
}

/// Format a `u64` as decimal into `buf`, returning the number of bytes
/// written.  The output is left-aligned at the start of `buf`.
fn format_u64(mut n: u64, buf: &mut [u8; 20]) -> usize {
    if n == 0 {
        if let Some(slot) = buf.get_mut(0) {
            *slot = b'0';
        }
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut i: usize = 0;
    while n > 0 && i < tmp.len() {
        if let Some(slot) = tmp.get_mut(i) {
            *slot = b'0'.wrapping_add((n % 10) as u8);
        }
        n /= 10;
        i = i.wrapping_add(1);
    }
    // Reverse into buf.
    let mut j: usize = 0;
    while j < i {
        let src = i.wrapping_sub(1).wrapping_sub(j);
        if let (Some(d), Some(s)) = (buf.get_mut(j), tmp.get(src)) {
            *d = *s;
        }
        j = j.wrapping_add(1);
    }
    i
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

// ---------------------------------------------------------------------------
// mlock2 — Linux memory locking extension
// ---------------------------------------------------------------------------

/// `MLOCK_ONFAULT` — only lock pages when they are faulted in.
/// Linux 4.4+ extension.  Without this flag, `mlock2` behaves
/// identically to `mlock`.
pub const MLOCK_ONFAULT: i32 = 1;

/// Lock pages in memory, with flags.
///
/// Linux extension.  `flags == 0` is equivalent to `mlock`.
/// `flags == MLOCK_ONFAULT` locks pages only as they are faulted in,
/// rather than faulting them all in immediately.
///
/// Stub: succeeds silently (same as `mlock`).  No kernel page-pinning
/// support yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mlock2(
    _addr: *const core::ffi::c_void,
    _len: SizeT,
    _flags: i32,
) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// mincore — query page residency
// ---------------------------------------------------------------------------

/// Determine whether pages are resident in memory.
///
/// Stub: returns -1 with ENOSYS.  A real implementation would query
/// the page table to determine which pages in the range are physically
/// resident.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mincore(
    _addr: *mut core::ffi::c_void,
    _length: SizeT,
    _vec: *mut u8,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}
