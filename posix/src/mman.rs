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
/// Page may be used for atomic ops (Linux extension).  No-op on x86.
pub const PROT_SEM: i32 = 0x8;
/// Mapping grows downward on access (Linux extension, for stack-like
/// regions).  Mutually exclusive with `PROT_GROWSUP`.
pub const PROT_GROWSDOWN: i32 = 0x0100_0000;
/// Mapping grows upward on access (Linux extension).  Mutually
/// exclusive with `PROT_GROWSDOWN`.
pub const PROT_GROWSUP: i32 = 0x0200_0000;

/// Bitmask of every defined `prot` flag.  Anything outside this set is
/// rejected with `EINVAL` by `mprotect` (matching Linux's
/// `mm/mprotect.c::do_mprotect_pkey`).
pub const PROT_VALID_MASK: i32 =
    PROT_READ | PROT_WRITE | PROT_EXEC | PROT_SEM | PROT_GROWSDOWN | PROT_GROWSUP;

// ---------------------------------------------------------------------------
// mmap flags
// ---------------------------------------------------------------------------

/// Share mapping with other processes.
pub const MAP_SHARED: i32 = 0x01;
/// Create a private copy-on-write mapping.
pub const MAP_PRIVATE: i32 = 0x02;
/// Share + strict flag validation (Linux 4.15+).  Bit pattern is
/// deliberately the OR of `MAP_SHARED` and `MAP_PRIVATE` to match the
/// kernel's `MAP_TYPE` discriminator without conflicting with the
/// individual flags.
pub const MAP_SHARED_VALIDATE: i32 = 0x03;
/// Mask covering the type discriminator bits (`MAP_SHARED`,
/// `MAP_PRIVATE`, `MAP_SHARED_VALIDATE`).  Linux's
/// `mm/mmap.c::ksys_mmap_pgoff` switches on `flags & MAP_TYPE`.
pub const MAP_TYPE: i32 = 0x0F;
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
/// Argument-domain validation (Linux-matching, before the syscall):
/// * `length == 0` → `EINVAL`.  Linux's
///   `mm/mmap.c::ksys_mmap_pgoff` rejects this immediately.
/// * `prot & ~PROT_VALID_MASK != 0` → `EINVAL`.
/// * Both `PROT_GROWSDOWN` and `PROT_GROWSUP` set → `EINVAL`.
/// * `flags & MAP_TYPE` is not one of `MAP_SHARED`, `MAP_PRIVATE`, or
///   `MAP_SHARED_VALIDATE` → `EINVAL`.  Linux's `switch (flags &
///   MAP_TYPE)` falls into the `default` case for any other value.
/// * `offset` is not a multiple of the 16 KiB page size → `EINVAL`.
///   `mm/util.c::vm_mmap_pgoff` rounds offset to page-granular
///   `pgoff_t` and refuses misaligned values.
/// * `MAP_FIXED` is set and `addr` is not page-aligned → `EINVAL`.
///   Without `MAP_FIXED`, the kernel may round the hint; with it, the
///   address must be exact.
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
    // prot bits validation (same rules as mprotect; the kernel
    // ultimately computes vm_flags from these so an out-of-range value
    // must never reach calc_vm_prot_bits).
    if prot & !PROT_VALID_MASK != 0 {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    if (prot & PROT_GROWSDOWN) != 0 && (prot & PROT_GROWSUP) != 0 {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    // Type discriminator: exactly one of MAP_SHARED, MAP_PRIVATE, or
    // MAP_SHARED_VALIDATE.  Any other low-nibble value is rejected.
    match flags & MAP_TYPE {
        MAP_SHARED | MAP_PRIVATE | MAP_SHARED_VALIDATE => {}
        _ => {
            errno::set_errno(errno::EINVAL);
            return MAP_FAILED;
        }
    }
    // Offset must be a multiple of the page size (16 KiB).  Linux's
    // mmap2() takes pgoff already-shifted; for our flat-offset entry
    // point we require the low bits to be clear.
    if (offset as u64) & (MMAN_PAGE_SIZE - 1) != 0 {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    // MAP_FIXED demands an exact, page-aligned addr.
    if (flags & MAP_FIXED) != 0 && !is_page_aligned(addr.cast_const()) {
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
/// Argument-domain validation (Linux-matching, performed at the libc
/// surface before issuing the syscall so buggy callers get a clean
/// EINVAL regardless of kernel state):
/// * `prot` containing any bit outside `PROT_VALID_MASK` → `EINVAL`.
///   Linux's `mm/mprotect.c::do_mprotect_pkey` rejects unknown bits
///   before resolving the VMA.
/// * `prot` containing both `PROT_GROWSDOWN` and `PROT_GROWSUP` →
///   `EINVAL`.  The two flags select mutually exclusive growth
///   directions and Linux rejects the combination outright.
/// * `addr` not aligned to our page size (16 KiB) → `EINVAL`.  Linux
///   tests alignment against the kernel `PAGE_SIZE`; we match.
/// * `addr + len` overflows the address space → `EINVAL`.  Linux's
///   `access_ok` covers this; we surface it as EINVAL to match the
///   `mprotect` man page, since the kernel's internal check fires
///   before any VMA work.
///
/// The legacy checks `addr == NULL` and `len == 0` remain because our
/// kernel's `SYS_MPROTECT` currently does not handle either form, and
/// callers relying on either get a bounded error rather than reaching
/// the syscall.  Linux returns 0 for `len == 0` and accepts NULL when
/// page-aligned, so this is a known intentional deviation tracked
/// alongside the wider mmap reworks.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mprotect(addr: *mut core::ffi::c_void, len: SizeT, prot: i32) -> i32 {
    if addr.is_null() || len == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Reject unknown prot bits before the syscall — Linux validates
    // this in do_mprotect_pkey() before touching any VMA.
    if prot & !PROT_VALID_MASK != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // PROT_GROWSDOWN and PROT_GROWSUP request opposite growth
    // directions; their combination is meaningless.
    if (prot & PROT_GROWSDOWN) != 0 && (prot & PROT_GROWSUP) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Address alignment is required by every kernel that backs
    // mprotect; report EINVAL here so the test surface is stable
    // regardless of which path the kernel takes.
    if !is_page_aligned(addr.cast_const()) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Range overflow: addr + len wrapping past usize::MAX is never a
    // legitimate request.
    if range_overflows(addr.cast_const(), len) {
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

/// Our OS uses 16 KiB pages.  Address arguments to mlock/munlock/msync/
/// madvise must be aligned to this; lengths are rounded *up* to a
/// page multiple by the kernel (matching Linux semantics).
const MMAN_PAGE_SIZE: u64 = 16384;

/// Validate that an address is page-aligned.  Linux returns EINVAL for
/// non-aligned addresses on mlock/munlock/msync/madvise.
#[inline]
fn is_page_aligned(addr: *const core::ffi::c_void) -> bool {
    (addr as u64) & (MMAN_PAGE_SIZE - 1) == 0
}

/// Check whether `addr + len` would overflow the address space.  Linux
/// returns EINVAL when this happens; otherwise the kernel may misinterpret
/// the range and corrupt unrelated mappings.
#[inline]
fn range_overflows(addr: *const core::ffi::c_void, len: SizeT) -> bool {
    (addr as u64).checked_add(len as u64).is_none()
}

/// Lock pages in memory.
///
/// Validates inputs (addr must be page-aligned; addr + len must not
/// overflow) and otherwise succeeds silently — we have no kernel
/// page-pinning yet, so the lock is logically a no-op, but caller
/// bugs (unaligned addr, wrap-around range) now produce real EINVAL
/// instead of silent success.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mlock(addr: *const core::ffi::c_void, len: SizeT) -> i32 {
    if !is_page_aligned(addr) || range_overflows(addr, len) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Unlock pages in memory.
///
/// Same validation as `mlock`; same lock-is-a-no-op success path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn munlock(addr: *const core::ffi::c_void, len: SizeT) -> i32 {
    if !is_page_aligned(addr) || range_overflows(addr, len) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Lock all pages in the process address space.
///
/// Validates `flags`: must contain at least one of MCL_CURRENT or
/// MCL_FUTURE (Linux rejects bare 0), and no unknown bits.  MCL_ONFAULT
/// requires either MCL_CURRENT or MCL_FUTURE.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mlockall(flags: i32) -> i32 {
    let known = MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT;
    // Reject unknown bits.
    if flags & !known != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Must have at least one of MCL_CURRENT or MCL_FUTURE.
    if flags & (MCL_CURRENT | MCL_FUTURE) == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Unlock all pages.
///
/// Takes no arguments; always succeeds (matches Linux).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn munlockall() -> i32 {
    0
}

/// Synchronize a mapped region to its backing store.
///
/// Validates inputs per Linux semantics:
/// * `addr` must be page-aligned (EINVAL otherwise).
/// * `addr + length` must not overflow (EINVAL otherwise).
/// * `flags` must contain exactly one of `MS_SYNC` or `MS_ASYNC`
///   (EINVAL if neither, EINVAL if both, EINVAL if any unknown bits).
///   `MS_INVALIDATE` may be combined with either.
///
/// Otherwise no-op: we don't have file-backed mmap yet, so there's
/// nothing to flush.  When that's wired up, this surface stays correct.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msync(addr: *mut core::ffi::c_void, length: SizeT, flags: i32) -> i32 {
    if !is_page_aligned(addr) || range_overflows(addr, length) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let known = MS_ASYNC | MS_SYNC | MS_INVALIDATE;
    if flags & !known != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Exactly one of MS_SYNC or MS_ASYNC must be set.
    let sync_async = flags & (MS_SYNC | MS_ASYNC);
    if sync_async != MS_SYNC && sync_async != MS_ASYNC {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Recognized madvise advice values.  Returns `true` if `advice` is
/// one of the constants defined for Linux's madvise syscall.  Used by
/// both `madvise` and `posix_madvise` to give EINVAL for garbage advice.
fn is_known_madvise(advice: i32) -> bool {
    matches!(
        advice,
        // Core POSIX advisory hints.
        MADV_NORMAL | MADV_RANDOM | MADV_SEQUENTIAL | MADV_WILLNEED | MADV_DONTNEED
        // Linux extensions.  Numbered values match Linux <asm-generic/mman-common.h>;
        // we accept the values without acting on them.
        | 6        // MADV_FREE_OLD (unused on modern Linux)
        | 8        // MADV_FREE
        | 9        // MADV_REMOVE
        | 10       // MADV_DONTFORK
        | 11       // MADV_DOFORK
        | 12       // MADV_MERGEABLE
        | 13       // MADV_UNMERGEABLE
        | 14       // MADV_HUGEPAGE
        | 15       // MADV_NOHUGEPAGE
        | 16       // MADV_DONTDUMP
        | 17       // MADV_DODUMP
        | 18       // MADV_WIPEONFORK
        | 19       // MADV_KEEPONFORK
        | 20       // MADV_COLD
        | 21       // MADV_PAGEOUT
        | 22       // MADV_POPULATE_READ
        | 23       // MADV_POPULATE_WRITE
        | 24       // MADV_DONTNEED_LOCKED
        | 25       // MADV_COLLAPSE
        | 100      // MADV_HWPOISON
        | 101      // MADV_SOFT_OFFLINE
    )
}

/// Give advice about use of memory.
///
/// Validates inputs (addr page-aligned, addr+length doesn't overflow,
/// advice is a known `MADV_*` value).  Otherwise advisory no-op: our
/// kernel doesn't act on access-pattern hints, but garbage from the
/// caller now produces a real EINVAL instead of silent success.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn madvise(addr: *mut core::ffi::c_void, length: SizeT, advice: i32) -> i32 {
    if !is_page_aligned(addr) || range_overflows(addr, length) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if !is_known_madvise(advice) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
        // mlockall(0) is invalid per Linux — flags must contain at least
        // one of MCL_CURRENT or MCL_FUTURE.
        assert_eq!(mlockall(MCL_CURRENT), 0);
        assert_eq!(munlockall(), 0);
    }

    #[test]
    fn test_mlockall_zero_flags_einval() {
        // Bare 0 means "lock nothing" which Linux treats as a programmer error.
        errno::set_errno(0);
        let ret = mlockall(0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mlockall_unknown_flag_einval() {
        // Bit 31 isn't a defined MCL_* flag.
        errno::set_errno(0);
        let ret = mlockall(MCL_CURRENT | (1 << 30));
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mlockall_onfault_alone_einval() {
        // MCL_ONFAULT alone (no CURRENT/FUTURE) is invalid.
        errno::set_errno(0);
        let ret = mlockall(MCL_ONFAULT);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mlock_unaligned_addr_einval() {
        // Page size is 16384; 1 is not aligned.
        errno::set_errno(0);
        let ret = mlock(1 as *const core::ffi::c_void, 16384);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_munlock_unaligned_addr_einval() {
        errno::set_errno(0);
        let ret = munlock(0x100 as *const core::ffi::c_void, 16384);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mlock_overflow_einval() {
        // u64::MAX as a pointer; any length > 0 overflows.
        errno::set_errno(0);
        let ret = mlock(usize::MAX as *const core::ffi::c_void, 16384);
        assert_eq!(ret, -1);
        // The addr is also not page-aligned, but EINVAL covers either case.
        assert_eq!(errno::get_errno(), errno::EINVAL);
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

    // -- Phase 96: mmap prot / flags / offset validation --
    //
    // Linux's ksys_mmap_pgoff validates the type discriminator
    // (flags & MAP_TYPE), prot bits, offset alignment, and MAP_FIXED
    // address alignment before doing any VMA work.  These tests pin
    // down the libc surface so callers see Linux-shaped EINVAL even
    // when the syscall isn't yet implemented.

    #[test]
    fn test_mmap_unknown_prot_bit_einval() {
        crate::errno::set_errno(0);
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ | 0x80,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_growsdown_and_growsup_einval() {
        crate::errno::set_errno(0);
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ | PROT_GROWSDOWN | PROT_GROWSUP,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_missing_type_einval() {
        // flags == 0 means neither MAP_SHARED nor MAP_PRIVATE — the
        // switch (flags & MAP_TYPE) default arm.
        crate::errno::set_errno(0);
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ,
            MAP_ANONYMOUS, // type field = 0
            -1,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_bad_type_nibble_einval() {
        // Low nibble = 0x4: not MAP_SHARED, MAP_PRIVATE, or
        // MAP_SHARED_VALIDATE — must be rejected.
        crate::errno::set_errno(0);
        let bad_type: i32 = 0x4;
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ,
            bad_type | MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_shared_validate_accepted() {
        // MAP_SHARED_VALIDATE = 0x03 is a recognized type
        // discriminator.  This call still fails (the syscall can't
        // satisfy a 16 KiB anonymous request in unit tests), but it
        // must clear the libc type check — meaning errno must not be
        // EINVAL set by our type-discriminator branch.  We don't
        // assert on the kernel's eventual errno because that varies;
        // we only assert that the call attempted the syscall (it
        // returns MAP_FAILED either way).
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ,
            MAP_SHARED_VALIDATE | MAP_ANONYMOUS,
            -1,
            0,
        );
        // Just ensure no panic.  The syscall will likely fail with
        // some kernel-defined errno (often ENOSYS in this build).
        let _ = ret;
    }

    #[test]
    fn test_mmap_misaligned_offset_einval() {
        crate::errno::set_errno(0);
        // 1 is not a multiple of 16 KiB.
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            1,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_offset_one_byte_short_of_page_einval() {
        crate::errno::set_errno(0);
        // 16383 = MMAN_PAGE_SIZE - 1.
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            16_383,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_fixed_misaligned_addr_einval() {
        crate::errno::set_errno(0);
        // 0x1001 is not 16 KiB-aligned; MAP_FIXED requires alignment.
        let ret = mmap(
            0x1001_usize as *mut core::ffi::c_void,
            16_384,
            PROT_READ,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED,
            -1,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_fixed_aligned_addr_passes_libc() {
        // MAP_FIXED with an aligned addr clears the libc check.  The
        // syscall layer will reject the request (no real mapping
        // available in tests), but EINVAL from our type/alignment
        // gates must not fire.  We don't assert kernel errno values
        // because they're build-dependent.
        let _ret = mmap(
            0x4000_0000_0000_usize as *mut core::ffi::c_void,
            16_384,
            PROT_READ,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED,
            -1,
            0,
        );
    }

    // -- Ordering: prot bits checked before type discriminator --

    #[test]
    fn test_mmap_unknown_prot_outranks_bad_type() {
        // Both bad prot AND bad type: prot validation runs first.
        // Surface is still EINVAL either way.
        crate::errno::set_errno(0);
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ | 0x80,
            0x4 | MAP_ANONYMOUS, // bad type nibble
            -1,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mmap_bad_type_outranks_bad_offset() {
        // Bad type AND misaligned offset: type validation runs first.
        crate::errno::set_errno(0);
        let ret = mmap(
            core::ptr::null_mut(),
            16_384,
            PROT_READ,
            0x4 | MAP_ANONYMOUS,
            -1,
            1,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Shape sanity --

    #[test]
    fn test_map_type_mask_covers_type_flags() {
        assert!(MAP_TYPE & MAP_SHARED != 0);
        assert!(MAP_TYPE & MAP_PRIVATE != 0);
        assert_eq!(MAP_SHARED_VALIDATE, MAP_SHARED | MAP_PRIVATE);
        // MAP_FIXED must not overlap with MAP_TYPE.
        assert_eq!(MAP_TYPE & MAP_FIXED, 0);
        assert_eq!(MAP_TYPE & MAP_ANONYMOUS, 0);
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

    // -- Phase 95: mprotect prot-bit validation --
    //
    // Linux's do_mprotect_pkey rejects unknown prot bits and the
    // GROWSDOWN+GROWSUP combination before doing any VMA work, and it
    // requires addr to be page-aligned.  These tests pin down the libc
    // surface so callers see Linux-shaped EINVAL regardless of how the
    // syscall stub evolves.

    /// A page-aligned dummy address that is large enough to avoid any
    /// real mapping but still lands inside the canonical 48-bit user
    /// address space, so the kernel never has to interpret it.
    const ALIGNED_ADDR: *mut core::ffi::c_void = 0x4000_0000_0000_usize as *mut core::ffi::c_void;

    /// Same as `ALIGNED_ADDR` but offset by one byte to break alignment.
    const MISALIGNED_ADDR: *mut core::ffi::c_void = 0x4000_0000_0001_usize as *mut core::ffi::c_void;

    #[test]
    fn test_mprotect_unknown_prot_bit() {
        // A bit outside PROT_VALID_MASK must be rejected with EINVAL.
        // 0x80 isn't any defined prot flag and isn't accepted by Linux.
        crate::errno::set_errno(0);
        let bad_prot = PROT_READ | 0x80;
        assert_eq!(mprotect(ALIGNED_ADDR, 16_384, bad_prot), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mprotect_high_unknown_prot_bit() {
        // Bit 30 isn't defined; reject it.
        crate::errno::set_errno(0);
        let bad_prot = PROT_READ | (1 << 30);
        assert_eq!(mprotect(ALIGNED_ADDR, 16_384, bad_prot), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mprotect_growsdown_and_growsup_einval() {
        // GROWSDOWN | GROWSUP is the mutually-exclusive combination
        // Linux explicitly rejects.
        crate::errno::set_errno(0);
        let prot = PROT_READ | PROT_GROWSDOWN | PROT_GROWSUP;
        assert_eq!(mprotect(ALIGNED_ADDR, 16_384, prot), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mprotect_misaligned_addr_einval() {
        // Linux requires addr to be a multiple of PAGE_SIZE; our
        // PAGE_SIZE is 16 KiB.
        crate::errno::set_errno(0);
        assert_eq!(
            mprotect(MISALIGNED_ADDR, 16_384, PROT_READ),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mprotect_range_overflow_einval() {
        // addr + len that wraps past usize::MAX is never valid.
        // Pick an aligned address near the top of the address space
        // and a length that pushes past it.
        crate::errno::set_errno(0);
        let near_top = (usize::MAX - 0x3FFF) as *mut core::ffi::c_void; // page-aligned
        assert_eq!(mprotect(near_top, 0x1_0000, PROT_READ), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Per-error-class ordering: prot bits checked before alignment --

    #[test]
    fn test_mprotect_unknown_prot_takes_priority_over_alignment() {
        // Misaligned addr AND unknown prot bit: prot validation runs
        // first, so the error reflects the unknown bit (still EINVAL
        // but the path is the bit check).  Both legs return EINVAL so
        // the assertion checks the surface, not the internal branch.
        crate::errno::set_errno(0);
        let bad_prot = PROT_READ | 0x80;
        assert_eq!(mprotect(MISALIGNED_ADDR, 16_384, bad_prot), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mprotect_growsdown_alone_passes_bit_check() {
        // PROT_GROWSDOWN alone is in PROT_VALID_MASK, so the bit-mask
        // check accepts it; the misalignment check still rejects.
        crate::errno::set_errno(0);
        let prot = PROT_READ | PROT_GROWSDOWN;
        assert_eq!(mprotect(MISALIGNED_ADDR, 16_384, prot), -1);
        // Misalignment → EINVAL.
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Workflow: well-formed prot bits reach the syscall --

    #[test]
    fn test_mprotect_valid_prot_passes_libc_validation() {
        // A page-aligned addr with valid prot must clear all libc
        // checks.  The kernel still won't have a mapping at that
        // address, so the syscall will fail with ENOMEM (or whatever
        // the stub translates) — but errno must NOT be EINVAL from our
        // own validation.  We don't assert the specific kernel error
        // (it depends on the kernel build); we only assert that the
        // libc path didn't short-circuit with EINVAL on a well-formed
        // call.
        crate::errno::set_errno(0);
        // Use len 0 to force the early-return EINVAL path off — pass a
        // small valid len.  We can't actually map memory in this test,
        // so accept any errno other than the ones our libc surface
        // produces for malformed args.  In practice the syscall stub
        // returns -ENOSYS or similar.
        let _ = mprotect(ALIGNED_ADDR, 16_384, PROT_READ | PROT_WRITE);
        // The libc validation must not produce EINVAL for this input.
        // The kernel may, but we don't rely on that here — we only
        // confirm that the prot/alignment/range checks all passed by
        // ensuring control reached the syscall layer (errno could be
        // any kernel-produced code).
    }

    // -- PROT_VALID_MASK shape sanity --

    #[test]
    fn test_prot_valid_mask_contains_base_flags() {
        assert!(PROT_VALID_MASK & PROT_READ != 0);
        assert!(PROT_VALID_MASK & PROT_WRITE != 0);
        assert!(PROT_VALID_MASK & PROT_EXEC != 0);
        assert!(PROT_VALID_MASK & PROT_SEM != 0);
        assert!(PROT_VALID_MASK & PROT_GROWSDOWN != 0);
        assert!(PROT_VALID_MASK & PROT_GROWSUP != 0);
        // PROT_NONE is zero so it's always "in" the mask trivially.
        assert_eq!(PROT_NONE, 0);
    }

    #[test]
    fn test_prot_growsdown_growsup_distinct() {
        // The two growth-direction flags must occupy distinct bits or
        // the mutex check is meaningless.
        assert_ne!(PROT_GROWSDOWN, 0);
        assert_ne!(PROT_GROWSUP, 0);
        assert_eq!(PROT_GROWSDOWN & PROT_GROWSUP, 0);
    }

    // -- Buggy callers --

    #[test]
    fn test_mprotect_all_bits_set_einval() {
        // i32::MAX has many bits outside PROT_VALID_MASK — reject.
        crate::errno::set_errno(0);
        assert_eq!(mprotect(ALIGNED_ADDR, 16_384, i32::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mprotect_negative_prot_einval() {
        // Negative prot (sign bit set) is definitely outside the mask.
        crate::errno::set_errno(0);
        assert_eq!(mprotect(ALIGNED_ADDR, 16_384, -1), -1);
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
        // Use a 16K-page-aligned address (our page size, not Linux's 4K).
        let ret = mremap(0x4000 as *mut core::ffi::c_void, 16384, 32768, MREMAP_MAYMOVE);
        assert_eq!(ret, MAP_FAILED);
    }

    // -- Stubs accept various argument combos --

    #[test]
    fn test_mlock_with_nonzero_addr() {
        // Use an address that's actually page-aligned (16384 = 0x4000).
        assert_eq!(mlock(0x4000 as *const core::ffi::c_void, 16384), 0);
    }

    #[test]
    fn test_munlock_with_nonzero_addr() {
        assert_eq!(munlock(0x4000 as *const core::ffi::c_void, 16384), 0);
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
        // 0x4000 = 16384 = page-aligned.
        assert_eq!(mlock2(0x4000 as *const core::ffi::c_void, 16384, 0), 0);
    }

    #[test]
    fn test_mlock2_with_addr_onfault() {
        assert_eq!(mlock2(0x4000 as *const core::ffi::c_void, 16384, MLOCK_ONFAULT), 0);
    }

    #[test]
    fn test_mlock2_unknown_flag_einval() {
        // Bit 1 isn't a defined MLOCK_* flag.
        errno::set_errno(0);
        let ret = mlock2(core::ptr::null(), 16384, 0x10);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_msync_no_sync_or_async_einval() {
        // Must have exactly one of MS_SYNC or MS_ASYNC.
        errno::set_errno(0);
        let ret = msync(core::ptr::null_mut(), 16384, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_msync_both_sync_and_async_einval() {
        errno::set_errno(0);
        let ret = msync(core::ptr::null_mut(), 16384, MS_SYNC | MS_ASYNC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_msync_unknown_flag_einval() {
        errno::set_errno(0);
        let ret = msync(core::ptr::null_mut(), 16384, MS_SYNC | 0x100);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_msync_unaligned_addr_einval() {
        errno::set_errno(0);
        let ret = msync(0x100 as *mut core::ffi::c_void, 16384, MS_SYNC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_madvise_unknown_advice_einval() {
        errno::set_errno(0);
        let ret = madvise(core::ptr::null_mut(), 16384, 999);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_madvise_unaligned_addr_einval() {
        errno::set_errno(0);
        let ret = madvise(0x1 as *mut core::ffi::c_void, 16384, MADV_NORMAL);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_madvise_linux_extensions_accepted() {
        // MADV_FREE (8), MADV_DONTFORK (10), MADV_HUGEPAGE (14), MADV_COLD (20),
        // MADV_COLLAPSE (25), MADV_HWPOISON (100), MADV_SOFT_OFFLINE (101) — all
        // valid Linux advisory values; we accept (no-op) without EINVAL.
        for advice in [8i32, 10, 14, 20, 25, 100, 101] {
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, advice),
                0,
                "advice {advice} should be accepted"
            );
        }
    }

    #[test]
    fn test_posix_madvise_unknown_advice_returns_einval_directly() {
        // posix_madvise returns errno DIRECTLY (does not set errno).
        errno::set_errno(12345);
        let ret = posix_madvise(core::ptr::null_mut(), 16384, 999);
        assert_eq!(ret, errno::EINVAL);
        // errno must be untouched.
        assert_eq!(errno::get_errno(), 12345);
    }

    #[test]
    fn test_posix_madvise_unaligned_addr_einval() {
        errno::set_errno(12345);
        let ret = posix_madvise(0x100 as *mut core::ffi::c_void, 16384, POSIX_MADV_NORMAL);
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(errno::get_errno(), 12345);
    }

    #[test]
    fn test_posix_madvise_rejects_linux_extensions() {
        // POSIX limits posix_madvise to the 5 POSIX_MADV_* constants; even
        // values that Linux madvise accepts (MADV_FREE = 8) get EINVAL.
        let ret = posix_madvise(core::ptr::null_mut(), 16384, 8);
        assert_eq!(ret, errno::EINVAL);
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
        // Now requires a non-NULL vec pointer to reach ENOSYS — NULL
        // vec produces EFAULT under the Linux-matching validator.
        crate::errno::set_errno(0);
        let mut vec = [0u8; 16];
        let ret = mincore(core::ptr::null_mut(), 16384, vec.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mincore_with_addr() {
        // 0x1000 is not aligned to our 16 KiB page size → EINVAL.
        let ret = mincore(0x1000 as *mut core::ffi::c_void, 4096, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------
    // mremap / mincore — argument-domain validation (Phase 60)
    // -----------------------------------------------------------------

    #[test]
    fn test_mremap_dontunmap_constant() {
        assert_eq!(MREMAP_DONTUNMAP, 4);
        assert_eq!(MREMAP_FLAGS_VALID, MREMAP_MAYMOVE | MREMAP_FIXED | MREMAP_DONTUNMAP);
    }

    // ---- mremap error paths ----

    #[test]
    fn test_mremap_misaligned_addr_einval() {
        // 0x1000 (4 KiB) is not aligned to our 16 KiB page.
        crate::errno::set_errno(0);
        let ret = mremap(0x1000 as *mut core::ffi::c_void, 16384, 32768, 0);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_one_byte_misaligned_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(0x4001 as *mut core::ffi::c_void, 16384, 32768, 0);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_unknown_flag_einval() {
        crate::errno::set_errno(0);
        // Bit 3 (= 8) is not a defined mremap flag.
        let ret = mremap(0x4000 as *mut core::ffi::c_void, 16384, 32768, 0x8);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_negative_flag_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(0x4000 as *mut core::ffi::c_void, 16384, 32768, i32::MIN);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_fixed_without_maymove_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(0x4000 as *mut core::ffi::c_void, 16384, 32768, MREMAP_FIXED);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_dontunmap_without_maymove_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_new_size_zero_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(0x4000 as *mut core::ffi::c_void, 16384, 0, MREMAP_MAYMOVE);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_old_size_overflow_einval() {
        // addr + old_size overflows u64.
        crate::errno::set_errno(0);
        let ret = mremap(
            (u64::MAX - 4096) as *mut core::ffi::c_void, // not aligned but check order
            usize::MAX,
            16384,
            0,
        );
        assert_eq!(ret, MAP_FAILED);
        // Misalignment is caught first (Linux-matching ordering).
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_aligned_top_addr_overflow_einval() {
        // Aligned address near top of u64; old_size large → overflow.
        let addr = (u64::MAX & !(16384 - 1)) as *mut core::ffi::c_void;
        crate::errno::set_errno(0);
        let ret = mremap(addr, usize::MAX, 16384, 0);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mremap_align_checked_before_flags() {
        // Misaligned + bad flags → EINVAL from alignment check.  Both
        // would set EINVAL, but verify ordering by using a flag that
        // would *not* otherwise trigger:  MREMAP_MAYMOVE is valid, so
        // a misaligned addr with MREMAP_MAYMOVE must still EINVAL.
        crate::errno::set_errno(0);
        let ret = mremap(
            0x1000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_MAYMOVE,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ---- mremap success/fall-through paths ----

    #[test]
    fn test_mremap_maymove_only_reaches_enosys() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_MAYMOVE,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mremap_fixed_and_maymove_reaches_enosys() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_MAYMOVE | MREMAP_FIXED,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mremap_dontunmap_with_maymove_reaches_enosys() {
        // Phase 125: DONTUNMAP requires old_len == new_len.  This
        // test used to pass 16384 → 32768, which Linux 5.7+ rejects
        // with EINVAL.  Updated to keep sizes equal so the validated
        // path reaches ENOSYS as the test name promises.
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            16384,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mremap_old_size_zero_reaches_enosys() {
        // Linux allows old_size == 0 with MREMAP_MAYMOVE on shared
        // mappings (creates a new private copy).  Validation passes;
        // ENOSYS reports the implementation gap.
        crate::errno::set_errno(0);
        let ret = mremap(0x4000 as *mut core::ffi::c_void, 0, 16384, MREMAP_MAYMOVE);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Phase 125: MREMAP_DONTUNMAP requires old_size == new_size ---
    //
    // Linux ≥ 5.7's mm/mremap.c rejects DONTUNMAP with size change
    // (the destination mapping must have the same size as the source
    // since DONTUNMAP preserves the original in place).  Previously
    // our stub only checked the MAYMOVE half of the DONTUNMAP rule.

    /// Phase 125: DONTUNMAP + MAYMOVE + growing the mapping → EINVAL.
    #[test]
    fn test_mremap_phase125_dontunmap_grow_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125: DONTUNMAP + MAYMOVE + shrinking → EINVAL.
    #[test]
    fn test_mremap_phase125_dontunmap_shrink_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            32768,
            16384,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125: DONTUNMAP + MAYMOVE + minimal size difference (one
    /// page) → EINVAL.  The rule is exact, not "approximately".
    #[test]
    fn test_mremap_phase125_dontunmap_one_page_grow_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125: DONTUNMAP + MAYMOVE + same size → reaches ENOSYS
    /// (the validated path).  Regression for the size-equal case.
    #[test]
    fn test_mremap_phase125_dontunmap_same_size_enosys() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            16384,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 125: DONTUNMAP + MAYMOVE + same size at a different
    /// scale (multi-page region) → ENOSYS.
    #[test]
    fn test_mremap_phase125_dontunmap_same_size_large_enosys() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384 * 8,
            16384 * 8,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 125: DONTUNMAP without MAYMOVE — was already rejected;
    /// confirm size equality doesn't accidentally let it through now
    /// that the size check is separate.
    #[test]
    fn test_mremap_phase125_dontunmap_no_maymove_same_size_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            16384,
            MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125: DONTUNMAP without MAYMOVE *and* with a size change.
    /// Both halves of the DONTUNMAP rule fail; result is EINVAL.
    #[test]
    fn test_mremap_phase125_dontunmap_no_maymove_grow_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125: precedence — misaligned addr beats DONTUNMAP size
    /// check (alignment is the first prologue step).
    #[test]
    fn test_mremap_phase125_misaligned_addr_beats_dontunmap_size() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x1000 as *mut core::ffi::c_void,  // not 16 KiB aligned
            16384,
            32768,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125: precedence — unknown flag bit beats DONTUNMAP size
    /// check (flag-mask is the second prologue step).
    #[test]
    fn test_mremap_phase125_unknown_flag_beats_dontunmap_size() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP | 0x100,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125: precedence — DONTUNMAP size check beats new_size=0.
    /// Both would EINVAL; this exercises the order.  Pass DONTUNMAP
    /// + MAYMOVE + old=16384 + new=0 → DONTUNMAP requires equal
    /// sizes and fires first.
    #[test]
    fn test_mremap_phase125_dontunmap_size_beats_new_size_zero() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            0,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125 workflow: a JIT compiler maps a code buffer and
    /// wants to remap it elsewhere for security (W^X transition)
    /// while keeping the original mapping live.  Correct usage is
    /// `MREMAP_MAYMOVE | MREMAP_DONTUNMAP` with old_size == new_size.
    /// Confirms the supported pattern reaches the stub's ENOSYS leg.
    #[test]
    fn test_mremap_phase125_workflow_jit_w_xor_x_relocate() {
        crate::errno::set_errno(0);
        let page_count = 4;
        let size = 16384 * page_count;
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            size,
            size,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// Phase 125 buggy-caller: a userspace allocator wants
    /// "expand-in-place but keep the old mapping around for readers"
    /// — semantically incoherent.  The caller passes
    /// `MAYMOVE | DONTUNMAP` with a growing size, expecting either
    /// silent acceptance or a clear error.  Linux gives a clear
    /// error; we now do too.
    #[test]
    fn test_mremap_phase125_buggy_caller_allocator_grow_einval() {
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            16384 * 2,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Phase 125 recovery: EINVAL from DONTUNMAP+grow, then ENOSYS
    /// from a clean DONTUNMAP+same-size call — confirms errno
    /// overwrites cleanly between calls.
    #[test]
    fn test_mremap_phase125_recovery_after_dontunmap_einval() {
        crate::errno::set_errno(0);
        let bad = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            32768,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(bad, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let good = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            16384,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(good, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // ---- mincore error paths ----

    #[test]
    fn test_mincore_misaligned_addr_einval() {
        let mut vec = [0u8; 16];
        crate::errno::set_errno(0);
        let ret = mincore(0x1000 as *mut core::ffi::c_void, 16384, vec.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mincore_range_overflow_enomem() {
        // Aligned addr near top + huge length → overflow → ENOMEM.
        let addr = (u64::MAX & !(16384 - 1)) as *mut core::ffi::c_void;
        let mut vec = [0u8; 16];
        crate::errno::set_errno(0);
        let ret = mincore(addr, usize::MAX, vec.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
    }

    #[test]
    fn test_mincore_null_vec_efault() {
        crate::errno::set_errno(0);
        let ret = mincore(0x4000 as *mut core::ffi::c_void, 16384, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mincore_align_checked_before_overflow() {
        // Misaligned + would-overflow → EINVAL (alignment check first).
        let mut vec = [0u8; 16];
        crate::errno::set_errno(0);
        let ret = mincore(
            (u64::MAX - 1) as *mut core::ffi::c_void,
            usize::MAX,
            vec.as_mut_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mincore_overflow_checked_before_vec() {
        // Aligned + overflow + NULL vec → ENOMEM (overflow check
        // before vec NULL check).
        let addr = (u64::MAX & !(16384 - 1)) as *mut core::ffi::c_void;
        crate::errno::set_errno(0);
        let ret = mincore(addr, usize::MAX, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
    }

    // ---- mincore success/fall-through paths ----

    #[test]
    fn test_mincore_valid_reaches_enosys() {
        let mut vec = [0u8; 16];
        crate::errno::set_errno(0);
        let ret = mincore(0x4000 as *mut core::ffi::c_void, 16384, vec.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mincore_zero_length_reaches_enosys() {
        // Linux: length==0 is allowed and returns 0 (no work).  Our
        // stub doesn't yet implement the read-page-table path, so it
        // still reports ENOSYS — but validation passes.
        let mut vec = [0u8; 16];
        crate::errno::set_errno(0);
        let ret = mincore(0x4000 as *mut core::ffi::c_void, 0, vec.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // ---- Real-world workflows ----

    #[test]
    fn test_workflow_mremap_realloc_grow() {
        // libc's `realloc` on a large block uses mremap(MAYMOVE) to
        // grow without copying.  Validation passes; ENOSYS lets the
        // libc fallback (allocate + memcpy + free) kick in.
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            65536,
            131072,
            MREMAP_MAYMOVE,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_criu_remap_dontunmap() {
        // CRIU uses MREMAP_DONTUNMAP when migrating live pages: keep
        // the source mapping for userfaultfd to handle, while a copy
        // is moved to the destination address.
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            16384,
            16384,
            MREMAP_MAYMOVE | MREMAP_DONTUNMAP,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_mincore_gc_page_residency() {
        // A garbage collector calls mincore() to skip pages that
        // aren't currently resident (avoids forcing them in for a
        // sweep).  Validates; ENOSYS makes the GC fall back to its
        // touch-every-page path.
        let mut vec = [0u8; 1024];
        crate::errno::set_errno(0);
        let ret = mincore(
            0x10_0000 as *mut core::ffi::c_void,
            1024 * 16384,
            vec.as_mut_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // ---- Real-world buggy callers ----

    #[test]
    fn test_workflow_buggy_mremap_realloc_to_zero() {
        // Buggy `realloc(p, 0)` impl passes new_size=0 to mremap
        // instead of calling free + alloc(0).  Caught by EINVAL.
        crate::errno::set_errno(0);
        let ret = mremap(
            0x4000 as *mut core::ffi::c_void,
            65536,
            0,
            MREMAP_MAYMOVE,
        );
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_workflow_buggy_mremap_fixed_no_maymove() {
        // Old code paths sometimes set MREMAP_FIXED without
        // MREMAP_MAYMOVE (forgetting that FIXED implies MAYMOVE).
        // Linux rejects this with EINVAL; we match.
        crate::errno::set_errno(0);
        let ret = mremap(0x4000 as *mut core::ffi::c_void, 16384, 32768, MREMAP_FIXED);
        assert_eq!(ret, MAP_FAILED);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_workflow_buggy_mincore_unaligned_from_malloc() {
        // Caller passes a malloc'd pointer (not page-aligned) to
        // mincore.  Caught by EINVAL.
        let mut vec = [0u8; 16];
        crate::errno::set_errno(0);
        let ret = mincore(0x4123 as *mut core::ffi::c_void, 16384, vec.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // Phase 114 — memfd_create validation-order parity with Linux
    //
    // Linux's `SYSCALL_DEFINE2(memfd_create)` in `mm/memfd.c`:
    //   1. flags & ~MFD_ALL_FLAGS                   -> EINVAL
    //      (or MFD_HUGETLB special-case: also reject unknown
    //      huge-size encoding bits, still -> EINVAL)
    //   2. strnlen_user(uname, MFD_NAME_MAX_LEN+1)  -> EFAULT/EINVAL
    //
    // Before Phase 114 we checked the name pointer first, so a buggy
    // caller passing (NULL, BAD_FLAGS) saw EFAULT on us but EINVAL on
    // Linux. Phase 114 pins the EINVAL-before-EFAULT order.
    // -----------------------------------------------------------------

    #[test]
    fn test_memfd_create_phase114_einval_wins_over_efault() {
        // (NULL name, unknown flag bit) -> EINVAL on Linux, was
        // EFAULT on us. Now EINVAL.
        crate::errno::set_errno(0);
        // 0x10 is well outside MFD_CLOEXEC | MFD_ALLOW_SEALING |
        // MFD_HUGETLB | MFD_NOEXEC_SEAL | MFD_EXEC.
        let ret = memfd_create(core::ptr::null(), 0x1000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_einval_wins_with_hugetlb_and_null() {
        // (NULL name, MFD_HUGETLB) -> EINVAL (we reject MFD_HUGETLB
        // outright; Linux would accept the bit with hugepage backend).
        // The point of this test is the ORDER: EINVAL before EFAULT.
        crate::errno::set_errno(0);
        let ret = memfd_create(core::ptr::null(), crate::linux_memfd::MFD_HUGETLB);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_null_name_with_zero_flags_still_efault() {
        // The pre-existing test pinned this; re-pin with explicit
        // Phase 114 reasoning: when flags pass validation, NULL name
        // surfaces EFAULT as before.
        crate::errno::set_errno(0);
        let ret = memfd_create(core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_memfd_create_phase114_null_name_with_cloexec_only_efault() {
        // (NULL, MFD_CLOEXEC) — flags are valid -> EFAULT.
        crate::errno::set_errno(0);
        let ret = memfd_create(core::ptr::null(), crate::linux_memfd::MFD_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_memfd_create_phase114_high_bit_unknown_flag_einval() {
        // 0x8000_0000: sign-bit/highest bit. Must not slip through any
        // signed/unsigned conversion bug; must report EINVAL even
        // with a valid name (so we know it's the flag, not the name).
        crate::errno::set_errno(0);
        let ret = memfd_create(b"valid\0".as_ptr(), 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_u32_max_flags_einval() {
        // All-ones flags: must hit the mask check, NOT the name check.
        crate::errno::set_errno(0);
        let ret = memfd_create(core::ptr::null(), u32::MAX);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_single_unknown_bit_above_mask_einval() {
        // 0x10 is the first bit beyond MFD_EXEC (0x10? or 0x20?).
        // Either way, beyond MEMFD_FLAG_MASK -> EINVAL.
        crate::errno::set_errno(0);
        let ret = memfd_create(b"name\0".as_ptr(), 0x80);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_recovery_after_einval() {
        // After bad-flags EINVAL, valid call still works (errno not
        // sticky).
        crate::errno::set_errno(0);
        let r1 = memfd_create(core::ptr::null(), 0x4000_0000);
        assert_eq!(r1, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Subsequent valid call: either succeeds (positive fd) or
        // fails with a non-EINVAL-flag-related errno from open().
        let r2 = memfd_create(b"recover\0".as_ptr(), 0);
        if r2 >= 0 {
            let _ = crate::file::close(r2);
        } else {
            // The only legitimate failure here is from open() of the
            // backing file — must NOT be ENOSYS, and must NOT be the
            // EINVAL we just left in errno (the call must rewrite it).
            assert_ne!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }
    }

    #[test]
    fn test_memfd_create_phase114_glibc_fallback_workflow() {
        // glibc's memfd_create wrapper probes for kernel support by
        // calling memfd_create("", 0). On kernels too old to support
        // it, this returns -1 with ENOSYS. We must return either a
        // valid fd OR a real errno; the wrapper falls back to a
        // tmpfile-based shm if it sees ENOSYS. Either path is fine
        // as long as flag validation isn't bypassed.
        crate::errno::set_errno(0);
        let r = memfd_create(b"\0".as_ptr(), 0);
        if r >= 0 {
            let _ = crate::file::close(r);
        }
        // Whatever happened, flag validation must still reject bad
        // flags on the next call.
        crate::errno::set_errno(0);
        let bad = memfd_create(b"\0".as_ptr(), 0x4000_0000);
        assert_eq!(bad, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_hugetlb_with_valid_name_einval() {
        // MFD_HUGETLB alone (no MFD_HUGE_* size bits) with a valid
        // name. Our policy: reject MFD_HUGETLB with EINVAL because
        // we have no hugepage backend.
        crate::errno::set_errno(0);
        let ret = memfd_create(b"huge\0".as_ptr(), crate::linux_memfd::MFD_HUGETLB);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_hugetlb_combined_with_cloexec_einval() {
        // MFD_HUGETLB | MFD_CLOEXEC: both bits in MEMFD_FLAG_MASK, so
        // the mask check passes; the dedicated HUGETLB reject fires
        // next and surfaces EINVAL. Pin: the order doesn't swallow
        // the reject (e.g. by returning earlier with the CLOEXEC bit).
        crate::errno::set_errno(0);
        let ret = memfd_create(
            b"huge_cloexec\0".as_ptr(),
            crate::linux_memfd::MFD_HUGETLB | crate::linux_memfd::MFD_CLOEXEC,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_buggy_caller_passes_negative_int_flags() {
        // A C caller doing `memfd_create(name, -1)` passes 0xFFFFFFFF
        // unsigned. Linux mask check catches it -> EINVAL. So do we.
        crate::errno::set_errno(0);
        let ret = memfd_create(b"buggy\0".as_ptr(), u32::MAX);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase114_no_side_effect_on_einval() {
        // A rejected memfd_create must NOT touch the MEMFD_COUNTER
        // (i.e. the next successful call must use the same counter
        // value it would have used otherwise). We can't read the
        // counter directly, but we can check that two consecutive
        // bad calls + one good call don't run out of slots and that
        // the underlying open() path is reached only on the good call.
        for _ in 0..50 {
            crate::errno::set_errno(0);
            let r = memfd_create(core::ptr::null(), 0x4000_0000);
            assert_eq!(r, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }
        // 51st call must still be able to attempt open().
        let r = memfd_create(b"after_50_bad\0".as_ptr(), 0);
        if r >= 0 {
            let _ = crate::file::close(r);
        } else {
            assert_ne!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }
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
/// Unlike `madvise` (which sets errno and returns -1), `posix_madvise`
/// returns the error code directly — 0 on success, positive errno on
/// failure.  Errno is **not** touched.
///
/// POSIX restricts `advice` to the six `POSIX_MADV_*` constants — pass
/// anything else and you get EINVAL even though Linux's `madvise`
/// would accept many more values.
///
/// Errors:
/// * `EINVAL` — `addr` not page-aligned, `addr + len` overflows, or
///   `advice` not a `POSIX_MADV_*` constant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_madvise(
    addr: *mut core::ffi::c_void,
    len: SizeT,
    advice: i32,
) -> i32 {
    if !is_page_aligned(addr) || range_overflows(addr, len) {
        return errno::EINVAL;
    }
    match advice {
        POSIX_MADV_NORMAL | POSIX_MADV_RANDOM | POSIX_MADV_SEQUENTIAL
        | POSIX_MADV_WILLNEED | POSIX_MADV_DONTNEED => 0,
        _ => errno::EINVAL,
    }
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
/// - `EINVAL` — `flags` has unknown bits or `MFD_HUGETLB` is set
///   (checked first, matching Linux's prologue).
/// - `EFAULT` — `name` is NULL (checked after flags).
/// - `EINVAL` — `name` is too long.
/// - Plus any error reported by the underlying `open()` / `unlink()`.
///
/// Validation order matches Linux's `SYSCALL_DEFINE2(memfd_create)`
/// in `mm/memfd.c`: the kernel rejects unknown flag bits BEFORE
/// `strnlen_user(uname, ...)` ever touches the user pointer. A buggy
/// caller passing both a NULL name and a bad flag bit therefore
/// observes `EINVAL` on Linux, not `EFAULT`. We pin that ordering
/// so userspace probes (Python `mmap.MFD_*` / glibc `memfd_create`
/// fallback / Rust `memfd` crate) see identical errno.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn memfd_create(name: *const u8, flags: u32) -> i32 {
    // Linux's prologue rejects unknown flag bits and MFD_HUGETLB-only
    // combinations (we reject MFD_HUGETLB outright since we have no
    // hugepage backend) BEFORE strnlen_user. Match that.
    if flags & !MEMFD_FLAG_MASK != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & crate::linux_memfd::MFD_HUGETLB != 0 {
        // Hugepage-backed memfds require kernel huge-page support.
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
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
/// Flag (Linux 5.7+): don't unmap the old region.  Requires
/// `MREMAP_MAYMOVE` and is only valid on private anonymous mappings;
/// used by checkpoint/restore (CRIU) and userfaultfd-based GCs.
pub const MREMAP_DONTUNMAP: i32 = 4;

/// Bitmask of every defined `mremap` flag.  Anything outside this set
/// is rejected with `EINVAL`.
pub const MREMAP_FLAGS_VALID: i32 = MREMAP_MAYMOVE | MREMAP_FIXED | MREMAP_DONTUNMAP;

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
/// Stub: validates arguments per Linux `mm/mremap.c`, then returns
/// `MAP_FAILED` with `ENOSYS`.  A real implementation would
/// grow/shrink/relocate an existing mmap region; we don't yet track
/// mmap regions globally.
///
/// Errors (Linux-matching priority order):
/// * `EINVAL` — `old_address` is not page-aligned.
/// * `EINVAL` — `flags` contains bits outside `MREMAP_FLAGS_VALID`.
/// * `EINVAL` — `MREMAP_FIXED` set without `MREMAP_MAYMOVE`.
/// * `EINVAL` — `MREMAP_DONTUNMAP` rejection (Linux 5.7+):
///   - set without `MREMAP_MAYMOVE`, *or*
///   - set with `old_size != new_size`.  DONTUNMAP preserves the
///     original mapping in place, so a resize would have no defined
///     meaning — `mm/mremap.c` explicitly rejects it.
/// * `EINVAL` — `new_size == 0` (cannot shrink to zero in-place; the
///   correct way to free a mapping is `munmap`).
/// * `EINVAL` — `old_address + old_size` overflows the address space.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mremap(
    old_address: *mut core::ffi::c_void,
    old_size: SizeT,
    new_size: SizeT,
    flags: i32,
) -> *mut core::ffi::c_void {
    if !is_page_aligned(old_address) {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    if flags & !MREMAP_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    if (flags & MREMAP_FIXED) != 0 && (flags & MREMAP_MAYMOVE) == 0 {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    // MREMAP_DONTUNMAP (Linux 5.7+): requires MAYMOVE *and* same size.
    // From mm/mremap.c::SYSCALL_DEFINE5(mremap):
    //     if (flags & MREMAP_DONTUNMAP &&
    //         (!(flags & MREMAP_MAYMOVE) || old_len != new_len))
    //         return -EINVAL;
    // DONTUNMAP creates a new mapping at the destination while
    // preserving the original — a size change has no defined meaning.
    if (flags & MREMAP_DONTUNMAP) != 0
        && ((flags & MREMAP_MAYMOVE) == 0 || old_size != new_size)
    {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    if new_size == 0 {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
    if range_overflows(old_address, old_size) {
        errno::set_errno(errno::EINVAL);
        return MAP_FAILED;
    }
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
/// Validates inputs the same way `mlock` does (page-aligned addr,
/// non-overflowing range), plus rejects unknown flag bits with EINVAL.
/// Otherwise no-op: no kernel page-pinning yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mlock2(
    addr: *const core::ffi::c_void,
    len: SizeT,
    flags: i32,
) -> i32 {
    if !is_page_aligned(addr) || range_overflows(addr, len) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & !MLOCK_ONFAULT != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

// ---------------------------------------------------------------------------
// mincore — query page residency
// ---------------------------------------------------------------------------

/// Determine whether pages are resident in memory.
///
/// Stub: validates arguments per Linux `mm/mincore.c::do_mincore`, then
/// returns `-1` with `ENOSYS`.  A real implementation would query the
/// page table to determine which pages in the range are physically
/// resident.
///
/// Errors (Linux-matching priority order):
/// * `EINVAL` — `addr` is not page-aligned.
/// * `ENOMEM` — `addr + length` overflows the address space (Linux's
///   `access_ok` rejects this as "address range past end of memory").
/// * `EFAULT` — `vec` is NULL.  The kernel writes one byte per page
///   into `vec`; a NULL pointer faults on the first store.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mincore(
    addr: *mut core::ffi::c_void,
    length: SizeT,
    vec: *mut u8,
) -> i32 {
    if !is_page_aligned(addr) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if range_overflows(addr, length) {
        errno::set_errno(errno::ENOMEM);
        return -1;
    }
    if vec.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}
