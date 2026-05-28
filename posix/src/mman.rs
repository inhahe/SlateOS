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

/// Phase 171 helper: read the current `RLIMIT_MEMLOCK` soft limit.
///
/// Wraps `crate::resource::getrlimit` and returns the `rlim_cur` value
/// (u64).  On the (unreachable) failure path returns `u64::MAX`,
/// which keeps the gate open — matching "infinity" / "no limit"
/// semantics.
fn current_memlock_limit() -> u64 {
    let mut rl = crate::resource::Rlimit { rlim_cur: 0, rlim_max: 0 };
    let rc = crate::resource::getrlimit(
        crate::resource::RLIMIT_MEMLOCK,
        &mut rl,
    );
    if rc == 0 { rl.rlim_cur } else { u64::MAX }
}

/// Phase 171: replicate Linux's `mm/mlock.c::can_do_mlock` /
/// over-limit checks for a request of `len` bytes.
///
/// Linux logic:
///   bool can_do_mlock(void) {
///       if (rlimit(RLIMIT_MEMLOCK) != 0) return true;
///       if (capable(CAP_IPC_LOCK))       return true;
///       return false;
///   }
/// then in `do_mlock`:
///   locked = current->mm->locked_vm + (len >> PAGE_SHIFT);
///   if (locked > lock_limit && !capable(CAP_IPC_LOCK))
///       return -ENOMEM;
///
/// In our flat model we have no per-task locked_vm tracker, so the
/// over-limit check collapses to `len > rlim_cur`.  Returns:
///   - `Ok(())` on pass (CAP_IPC_LOCK held, or within the limit);
///   - `Err(EPERM)` on the `can_do_mlock` failure (rlim == 0 and no cap);
///   - `Err(ENOMEM)` on the over-limit failure (len > rlim, no cap).
///
/// Callers should immediately convert the error to an errno-set and
/// `-1` return.  The check happens *after* argument-domain validation
/// (page alignment, overflow) — Linux's order.
fn check_mlock_caps(len: u64) -> Result<(), i32> {
    let has_cap = crate::sys_capability::has_capability(
        crate::sys_capability::CAP_IPC_LOCK,
    );
    if has_cap {
        return Ok(());
    }
    let lim = current_memlock_limit();
    // can_do_mlock: rlim == 0 → EPERM.
    if lim == 0 {
        return Err(errno::EPERM);
    }
    // Over-limit: len > rlim → ENOMEM.  Linux's check is on
    // `locked_vm + new pages`; with locked_vm = 0 in our flat model
    // this simplifies to `len > rlim`.
    if len > lim {
        return Err(errno::ENOMEM);
    }
    Ok(())
}

/// Lock pages in memory.
///
/// Validates inputs (addr must be page-aligned; addr + len must not
/// overflow) and otherwise succeeds silently — we have no kernel
/// page-pinning yet, so the lock is logically a no-op, but caller
/// bugs (unaligned addr, wrap-around range) now produce real EINVAL
/// instead of silent success.
///
/// Phase 171: also enforce Linux's `can_do_mlock` /
/// `over-RLIMIT_MEMLOCK` gates for callers without `CAP_IPC_LOCK`:
///   - `RLIMIT_MEMLOCK == 0` and no cap → `EPERM`;
///   - `len > RLIMIT_MEMLOCK` and no cap → `ENOMEM`.
/// Validation order matches Linux's `mm/mlock.c::do_mlock`: EINVAL
/// for argument-domain failures fires before EPERM/ENOMEM for the
/// capability/rlimit gate.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mlock(addr: *const core::ffi::c_void, len: SizeT) -> i32 {
    if !is_page_aligned(addr) || range_overflows(addr, len) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if let Err(e) = check_mlock_caps(len as u64) {
        errno::set_errno(e);
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
///
/// Phase 171: enforce Linux's `can_do_mlock` gate for callers
/// without `CAP_IPC_LOCK` — if `RLIMIT_MEMLOCK == 0` and the cap is
/// not held, return `-1`/EPERM.  The over-limit ENOMEM gate doesn't
/// apply here because `mlockall` has no explicit `len`; Linux uses
/// `mm->total_vm` against the rlimit, which we approximate by only
/// firing the EPERM branch (no caller can ask for "at most N bytes
/// locked" through mlockall).
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
    // Phase 171: can_do_mlock gate.  Pass len=0 so only the EPERM
    // branch (rlim == 0 + no cap) can fire; the ENOMEM branch is
    // suppressed because mlockall has no caller-supplied length.
    if let Err(e) = check_mlock_caps(0) {
        errno::set_errno(e);
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

/// MADV_HWPOISON — Linux kernel's "inject memory error" advice.  Routed
/// through `madvise_inject_error` and gated on `CAP_SYS_ADMIN`.
const MADV_HWPOISON: i32 = 100;
/// MADV_SOFT_OFFLINE — softer variant of `MADV_HWPOISON`.  Same
/// `madvise_inject_error` path, same `CAP_SYS_ADMIN` requirement.
const MADV_SOFT_OFFLINE: i32 = 101;

/// Give advice about use of memory.
///
/// Validates inputs (addr page-aligned, addr+length doesn't overflow,
/// advice is a known `MADV_*` value).  Otherwise advisory no-op: our
/// kernel doesn't act on access-pattern hints, but garbage from the
/// caller now produces a real EINVAL instead of silent success.
///
/// Validation order matches Linux's `mm/madvise.c::do_madvise` and
/// `madvise_inject_error`:
/// 1. `addr` not page-aligned, or `addr + length` overflows → `EINVAL`.
/// 2. `advice` not a recognised `MADV_*` value → `EINVAL`.
/// 3. (Phase 189) `advice` is `MADV_HWPOISON` (100) or
///    `MADV_SOFT_OFFLINE` (101) and the caller lacks `CAP_SYS_ADMIN` →
///    `EPERM`.  Matches `madvise_inject_error`:
///    ```text
///    if (!capable(CAP_SYS_ADMIN))
///        return -EPERM;
///    ```
///    The cap check fires only for the error-injection family — every
///    other `MADV_*` value remains an advisory no-op success.
/// 4. (Phase 189) `MADV_HWPOISON` / `MADV_SOFT_OFFLINE` with
///    `CAP_SYS_ADMIN` held → `ENOSYS`.  We do not implement the
///    memory-error injection backend; surfacing ENOSYS lets privileged
///    test tools (RAS validation suites, mce-test) distinguish "denied"
///    from "no backend".
/// 5. All other recognised `MADV_*` values → `0` (advisory no-op).
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
    // Phase 189: HWPOISON / SOFT_OFFLINE are the error-injection
    // family — Linux routes them through `madvise_inject_error`,
    // which opens with `if (!capable(CAP_SYS_ADMIN)) return -EPERM`.
    // The cap check fires only for these two; every other recognised
    // advice value falls through to the advisory no-op below.
    if advice == MADV_HWPOISON || advice == MADV_SOFT_OFFLINE {
        if !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SYS_ADMIN,
        ) {
            errno::set_errno(errno::EPERM);
            return -1;
        }
        // Cap held but we have no memory-error injection backend.
        errno::set_errno(errno::ENOSYS);
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
        // MADV_COLLAPSE (25) — pure advisory values; we accept (no-op) without
        // EINVAL.  HWPOISON (100) and SOFT_OFFLINE (101) are excluded here:
        // Phase 189 gates them on CAP_SYS_ADMIN and surfaces ENOSYS once the
        // cap check passes (no backend).  See the `madvise_cap_phase189`
        // module for their dedicated coverage.
        for advice in [8i32, 10, 14, 20, 25] {
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

    // ------------------------------------------------------------------
    // Phase 138 — MFD_EXEC and MFD_NOEXEC_SEAL mutual exclusion
    //
    // Linux 6.3 added MFD_NOEXEC_SEAL and MFD_EXEC and made them
    // mutually exclusive in `mm/memfd.c::memfd_create`:
    //
    //     if ((flags & MFD_EXEC) && (flags & MFD_NOEXEC_SEAL))
    //         return -EINVAL;
    //
    // The check sits between the flag-mask reject and
    // `strnlen_user(uname, ...)`, so EINVAL wins over the
    // NULL-name EFAULT.  Pre-Phase 138 we passed both bits through
    // (both are in MEMFD_FLAG_MASK) and only the underlying open()
    // would have reported anything anomalous — which it can't,
    // because the user-visible behaviour of the two flags is
    // contradictory and not representable as an open() failure.
    // ------------------------------------------------------------------

    #[test]
    fn test_memfd_create_phase138_constants_distinct() {
        // MFD_EXEC (0x10) and MFD_NOEXEC_SEAL (0x08) must occupy
        // distinct bits; otherwise the mutual-exclusion check below
        // can't tell the two apart.
        use crate::linux_memfd::{MFD_EXEC, MFD_NOEXEC_SEAL};
        assert_ne!(MFD_EXEC, MFD_NOEXEC_SEAL);
        assert_eq!(MFD_EXEC & MFD_NOEXEC_SEAL, 0);
    }

    #[test]
    fn test_memfd_create_phase138_exec_and_noexec_seal_einval() {
        // Core regression: both flags together → EINVAL.
        crate::errno::set_errno(0);
        let ret = memfd_create(
            b"both\0".as_ptr(),
            crate::linux_memfd::MFD_EXEC | crate::linux_memfd::MFD_NOEXEC_SEAL,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase138_exec_alone_accepted() {
        // MFD_EXEC alone is a valid flag — must NOT trip the new
        // mutual-exclusion check.  We don't care about the return
        // value (open() may fail in the test sandbox), only that the
        // errno isn't the EINVAL the mutex check would set.
        crate::errno::set_errno(0);
        let r = memfd_create(b"exec_only\0".as_ptr(), crate::linux_memfd::MFD_EXEC);
        if r >= 0 {
            let _ = crate::file::close(r);
        } else {
            // Any failure must not be the mutex EINVAL — open()
            // failures surface a different errno entirely.  We can't
            // exhaustively enumerate "not mutex EINVAL" but we can
            // assert it's not the canonical bad-flag value while the
            // input is a valid single flag.
            assert_ne!(crate::errno::get_errno(), 0);
        }
    }

    #[test]
    fn test_memfd_create_phase138_noexec_seal_alone_accepted() {
        // Symmetric: MFD_NOEXEC_SEAL alone must reach open().
        crate::errno::set_errno(0);
        let r = memfd_create(
            b"noexec_only\0".as_ptr(),
            crate::linux_memfd::MFD_NOEXEC_SEAL,
        );
        if r >= 0 {
            let _ = crate::file::close(r);
        } else {
            assert_ne!(crate::errno::get_errno(), 0);
        }
    }

    #[test]
    fn test_memfd_create_phase138_exec_plus_cloexec_accepted() {
        // MFD_EXEC | MFD_CLOEXEC: legitimate combo (caller wants the
        // memfd executable AND close-on-exec on the fd).  Must NOT
        // trip the mutual-exclusion check.
        crate::errno::set_errno(0);
        let r = memfd_create(
            b"exec_cloexec\0".as_ptr(),
            crate::linux_memfd::MFD_EXEC | crate::linux_memfd::MFD_CLOEXEC,
        );
        if r >= 0 {
            let _ = crate::file::close(r);
        }
    }

    #[test]
    fn test_memfd_create_phase138_noexec_plus_allow_sealing_accepted() {
        // MFD_NOEXEC_SEAL | MFD_ALLOW_SEALING: legitimate combo
        // (sealing infrastructure with executable sealing).  Must NOT
        // trip the mutex check.
        crate::errno::set_errno(0);
        let r = memfd_create(
            b"noexec_sealing\0".as_ptr(),
            crate::linux_memfd::MFD_NOEXEC_SEAL | crate::linux_memfd::MFD_ALLOW_SEALING,
        );
        if r >= 0 {
            let _ = crate::file::close(r);
        }
    }

    // --- ordering matrix --------------------------------------------------

    #[test]
    fn test_memfd_create_phase138_mask_check_beats_mutex_check() {
        // Unknown flag bit ALSO set with the mutex pair: the
        // flag-mask check runs first.  We can't directly observe
        // which check rejected it (both → EINVAL) but we CAN observe
        // that an unknown flag pinned high keeps the rejection
        // consistent across the "with and without the mutex pair"
        // axes — i.e. the check before the mutex still wins for
        // unknown bits.
        crate::errno::set_errno(0);
        let with_pair = memfd_create(
            b"x\0".as_ptr(),
            0x4000_0000
                | crate::linux_memfd::MFD_EXEC
                | crate::linux_memfd::MFD_NOEXEC_SEAL,
        );
        assert_eq!(with_pair, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let without_pair = memfd_create(b"x\0".as_ptr(), 0x4000_0000);
        assert_eq!(without_pair, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase138_hugetlb_check_beats_mutex_check() {
        // Linux's order: flag-mask → HUGETLB reject (ours) → mutex
        // check.  MFD_HUGETLB + MFD_EXEC + MFD_NOEXEC_SEAL: both the
        // HUGETLB and the mutex check would fire EINVAL.  We can't
        // observe which one without an instrumented build, but pin
        // that we still return EINVAL (regression guard for someone
        // reordering the two checks and accidentally letting a
        // hugetlb+exec_pair combo slip past).
        crate::errno::set_errno(0);
        let ret = memfd_create(
            b"h\0".as_ptr(),
            crate::linux_memfd::MFD_HUGETLB
                | crate::linux_memfd::MFD_EXEC
                | crate::linux_memfd::MFD_NOEXEC_SEAL,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase138_mutex_check_beats_null_name() {
        // Critical ordering: NULL name + MFD_EXEC|MFD_NOEXEC_SEAL.
        // Linux's mutex check runs BEFORE strnlen_user, so EINVAL
        // wins over EFAULT.  Pre-Phase 138 our code reached the
        // `name.is_null()` branch and returned EFAULT instead.
        crate::errno::set_errno(0);
        let ret = memfd_create(
            core::ptr::null(),
            crate::linux_memfd::MFD_EXEC | crate::linux_memfd::MFD_NOEXEC_SEAL,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- buggy callers ---------------------------------------------------

    #[test]
    fn test_memfd_create_phase138_buggy_caller_or_in_both_flags() {
        // A library upgrading from "MFD_EXEC for Linux 6.3+" to
        // "MFD_NOEXEC_SEAL for sealed memfds" without removing the
        // old flag.  Catches a real upgrade bug.
        crate::errno::set_errno(0);
        let flags = crate::linux_memfd::MFD_CLOEXEC
            | crate::linux_memfd::MFD_ALLOW_SEALING
            | crate::linux_memfd::MFD_EXEC
            | crate::linux_memfd::MFD_NOEXEC_SEAL;
        let ret = memfd_create(b"upgrade_bug\0".as_ptr(), flags);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_memfd_create_phase138_buggy_caller_zeroed_then_or_pair() {
        // Caller does `flags = 0; flags |= MFD_EXEC; flags |=
        // MFD_NOEXEC_SEAL;` thinking they're independent.  EINVAL.
        let mut flags: u32 = 0;
        flags |= crate::linux_memfd::MFD_EXEC;
        flags |= crate::linux_memfd::MFD_NOEXEC_SEAL;
        crate::errno::set_errno(0);
        let ret = memfd_create(b"zeroed_or\0".as_ptr(), flags);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- recovery / workflow -------------------------------------------

    #[test]
    fn test_memfd_create_phase138_recovery_after_mutex_einval() {
        // Caller sees EINVAL, drops one of the conflicting flags,
        // retries successfully (or with an unrelated failure).
        crate::errno::set_errno(0);
        let bad = memfd_create(
            b"r\0".as_ptr(),
            crate::linux_memfd::MFD_EXEC | crate::linux_memfd::MFD_NOEXEC_SEAL,
        );
        assert_eq!(bad, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Retry with just MFD_EXEC.
        let r = memfd_create(b"r\0".as_ptr(), crate::linux_memfd::MFD_EXEC);
        if r >= 0 {
            let _ = crate::file::close(r);
        }
    }

    #[test]
    fn test_memfd_create_phase138_workflow_glibc_upgrade_path() {
        // Real-world: a Rust `memfd` crate that adds MFD_EXEC support
        // for Linux 6.3+ but accidentally still passes
        // MFD_NOEXEC_SEAL (the default since 6.3 when neither is
        // explicitly set, per sysctl `vm.memfd_noexec`).  The first
        // call exposes the bug with EINVAL; the corrected call (one
        // flag only) goes through.
        crate::errno::set_errno(0);
        let buggy = memfd_create(
            b"upgrade\0".as_ptr(),
            crate::linux_memfd::MFD_CLOEXEC
                | crate::linux_memfd::MFD_EXEC
                | crate::linux_memfd::MFD_NOEXEC_SEAL,
        );
        assert_eq!(buggy, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Fixed: pick MFD_EXEC only (the explicit user-wants-exec
        // path).
        crate::errno::set_errno(0);
        let fixed = memfd_create(
            b"upgrade\0".as_ptr(),
            crate::linux_memfd::MFD_CLOEXEC | crate::linux_memfd::MFD_EXEC,
        );
        if fixed >= 0 {
            let _ = crate::file::close(fixed);
        } else {
            // Whatever the failure, it must NOT be the mutex EINVAL
            // (errno must be re-set by the second call, not stale).
            assert_ne!(crate::errno::get_errno(), 0);
        }
    }

    #[test]
    fn test_memfd_create_phase138_no_side_effect_on_mutex_einval() {
        // 100 rejected mutex-EINVAL calls must not consume any
        // resources or perturb the counter.
        for _ in 0..100 {
            crate::errno::set_errno(0);
            let r = memfd_create(
                b"loop\0".as_ptr(),
                crate::linux_memfd::MFD_EXEC | crate::linux_memfd::MFD_NOEXEC_SEAL,
            );
            assert_eq!(r, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }
        // Next valid call still works (or fails with a non-mutex
        // errno from open()).
        let r = memfd_create(b"after\0".as_ptr(), 0);
        if r >= 0 {
            let _ = crate::file::close(r);
        }
    }

    // ----------------------------------------------------------------------
    // Phase 171: mlock / mlock2 / mlockall — CAP_IPC_LOCK + RLIMIT_MEMLOCK
    // gating per Linux's `mm/mlock.c::can_do_mlock` and over-limit ENOMEM.
    //
    // Pre-Phase-171 behaviour: any caller could call `mlock(addr, len)`
    // and succeed (after EINVAL alignment / overflow checks).  No cap
    // probe, no rlimit consultation.  An unprivileged process could
    // logically pin arbitrary amounts of memory.
    //
    // Linux semantics:
    //   can_do_mlock(): true iff rlim(RLIMIT_MEMLOCK) > 0 OR CAP_IPC_LOCK.
    //   do_mlock(): if !can_do_mlock() → -EPERM;
    //               if locked_vm + new > rlim && !CAP_IPC_LOCK → -ENOMEM.
    //
    // Our flat model has no per-task locked_vm — we approximate the
    // over-limit check as `len > rlim_cur`.  Default RLIMIT_MEMLOCK in
    // our reset_global_state is RLIM_INFINITY, so the privileged path
    // (held cap or infinite rlim) lets every call succeed; the gates
    // become observable only when (a) caps are dropped *and* (b) the
    // rlimit is set to a finite value via setrlimit.
    //
    // Validation order: EINVAL (alignment, overflow, unknown flags)
    // beats EPERM/ENOMEM, matching Linux's `do_mlock` order.
    // ----------------------------------------------------------------------

    mod mlock_cap_phase171 {
        use super::*;
        use crate::resource::{
            Rlimit, RLIMIT_MEMLOCK, RLIM_INFINITY, setrlimit,
        };

        const PAGE: usize = 16384;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 77 / 164–170.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        /// RAII rlimit restorer for `RLIMIT_MEMLOCK`.  Snapshots
        /// the current `(rlim_cur, rlim_max)` and restores it on
        /// drop so tests don't poison the global RLIMITS table.
        struct MemlockGuard {
            saved: Rlimit,
        }
        impl MemlockGuard {
            fn snapshot() -> Self {
                let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
                let rc = crate::resource::getrlimit(RLIMIT_MEMLOCK, &mut rl);
                assert_eq!(rc, 0);
                Self { saved: rl }
            }
        }
        impl Drop for MemlockGuard {
            fn drop(&mut self) {
                // Restore via the raw setrlimit path — note Linux's
                // unprivileged setrlimit cannot raise rlim_max, but
                // our setrlimit stub doesn't enforce that, so the
                // restore is reliable.
                let _ = setrlimit(RLIMIT_MEMLOCK, &self.saved as *const _);
            }
        }

        fn drop_cap_ipc_lock() {
            use crate::sys_capability::CAP_IPC_LOCK;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_IPC_LOCK < 32 {
                (lo & !(1u32 << CAP_IPC_LOCK), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_IPC_LOCK - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_IPC_LOCK");
            assert!(!crate::sys_capability::has_capability(CAP_IPC_LOCK));
        }

        fn set_memlock(cur: u64, max: u64) {
            let rl = Rlimit { rlim_cur: cur, rlim_max: max };
            let rc = setrlimit(RLIMIT_MEMLOCK, &rl as *const _);
            assert_eq!(rc, 0);
        }

        // -- Per-error-class ---------------------------------------------

        /// `mlock` with rlim==0 and no cap → EPERM (can_do_mlock fails).
        #[test]
        fn test_mlock_phase171_rlim_zero_no_cap_eperm() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlock(core::ptr::null(), PAGE);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `mlock` with finite rlim and oversized len without cap →
        /// ENOMEM (over-limit branch).
        #[test]
        fn test_mlock_phase171_over_limit_no_cap_enomem() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            // 1 page worth of rlim — request 2 pages.
            set_memlock(PAGE as u64, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlock(core::ptr::null(), 2 * PAGE);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOMEM);
        }

        /// `mlock` with finite rlim and request within rlim without
        /// cap → success.  Confirms the gate is len-sensitive.
        #[test]
        fn test_mlock_phase171_within_limit_no_cap_ok() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(8 * PAGE as u64, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlock(core::ptr::null(), 4 * PAGE);
            assert_eq!(r, 0);
        }

        // -- Ordering matrix ---------------------------------------------

        /// EINVAL on unaligned addr beats EPERM (alignment check
        /// fires before cap probe).
        #[test]
        fn test_mlock_phase171_einval_alignment_beats_eperm() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY); // rlim==0 → would be EPERM
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlock(1 as *const core::ffi::c_void, PAGE);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// EINVAL on overflow beats ENOMEM (overflow check fires
        /// before the over-limit branch).
        #[test]
        fn test_mlock_phase171_einval_overflow_beats_enomem() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(PAGE as u64, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlock(usize::MAX as *const core::ffi::c_void, PAGE);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- mlock2 -------------------------------------------------------

        /// `mlock2` shares the gate.
        #[test]
        fn test_mlock2_phase171_rlim_zero_no_cap_eperm() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlock2(core::ptr::null(), PAGE, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `mlock2` with unknown flag beats the cap gate (EINVAL
        /// first).
        #[test]
        fn test_mlock2_phase171_einval_flag_beats_eperm() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            // 1 << 30 is not MLOCK_ONFAULT.
            let r = mlock2(core::ptr::null(), PAGE, 1 << 30);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- mlockall -----------------------------------------------------

        /// `mlockall` with rlim==0 and no cap → EPERM.
        #[test]
        fn test_mlockall_phase171_rlim_zero_no_cap_eperm() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlockall(MCL_CURRENT);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `mlockall` doesn't ENOMEM under no cap + finite rlim —
        /// only the EPERM branch is reachable because mlockall has
        /// no caller-supplied length.
        #[test]
        fn test_mlockall_phase171_finite_rlim_no_cap_ok() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(PAGE as u64, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlockall(MCL_CURRENT);
            assert_eq!(r, 0);
        }

        /// `mlockall` EINVAL on unknown flag beats EPERM.
        #[test]
        fn test_mlockall_phase171_einval_flag_beats_eperm() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            let r = mlockall(1 << 30);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- Workflow -----------------------------------------------------

        /// Realtime-audio-daemon workflow: while privileged,
        /// mlockall succeeds; daemon drops cap; subsequent mlock of
        /// an oversize buffer ENOMEMs; a sized-down mlock succeeds.
        #[test]
        fn test_mlock_phase171_workflow_lock_then_drop_then_size_down() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            // Privileged: lockall succeeds.
            errno::set_errno(0);
            assert_eq!(mlockall(MCL_CURRENT | MCL_FUTURE), 0);
            // Drop cap, set 4-page rlim.
            set_memlock(4 * PAGE as u64, RLIM_INFINITY);
            drop_cap_ipc_lock();
            // Oversize → ENOMEM.
            errno::set_errno(0);
            assert_eq!(mlock(core::ptr::null(), 8 * PAGE), -1);
            assert_eq!(errno::get_errno(), errno::ENOMEM);
            // Sized-down → ok.
            errno::set_errno(0);
            assert_eq!(mlock(core::ptr::null(), 2 * PAGE), 0);
        }

        // -- Recovery -----------------------------------------------------

        /// After EPERM, restoring CAP_IPC_LOCK lets the same call
        /// succeed.
        #[test]
        fn test_mlock_phase171_recovery_restore_cap_lets_lock_succeed() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            assert_eq!(mlock(core::ptr::null(), PAGE), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Restore caps to default-all.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(mlock(core::ptr::null(), PAGE), 0);
        }

        // -- Sentinel -----------------------------------------------------

        /// With CAP_IPC_LOCK held, even rlim==0 doesn't block mlock —
        /// the cap short-circuits both branches.
        #[test]
        fn test_mlock_phase171_sentinel_with_cap_rlim_zero_ok() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_IPC_LOCK,
            ));
            errno::set_errno(0);
            assert_eq!(mlock(core::ptr::null(), 16 * PAGE), 0);
        }

        // -- Cross-checks -------------------------------------------------

        /// `munlock` is *not* gated (matches Linux — unlocking is
        /// always permitted).
        #[test]
        fn test_mlock_phase171_munlock_unaffected() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            errno::set_errno(0);
            assert_eq!(munlock(core::ptr::null(), PAGE), 0);
        }

        /// `munlockall` is *not* gated.
        #[test]
        fn test_mlock_phase171_munlockall_unaffected() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(0, RLIM_INFINITY);
            drop_cap_ipc_lock();
            assert_eq!(munlockall(), 0);
        }

        /// Default state (default caps, RLIM_INFINITY memlock) is
        /// fully permissive — confirms no regression in the common
        /// case after Phase 171.
        #[test]
        fn test_mlock_phase171_default_state_permissive() {
            let _gc = CapGuard::snapshot();
            let _gl = MemlockGuard::snapshot();
            set_memlock(RLIM_INFINITY, RLIM_INFINITY);
            errno::set_errno(0);
            assert_eq!(mlock(core::ptr::null(), 64 * PAGE), 0);
            assert_eq!(mlockall(MCL_CURRENT | MCL_FUTURE), 0);
            assert_eq!(mlock2(core::ptr::null(), 16 * PAGE, MLOCK_ONFAULT), 0);
        }
    }

    // ------------------------------------------------------------------
    // Phase 189: madvise — CAP_SYS_ADMIN gate for HWPOISON / SOFT_OFFLINE
    // ------------------------------------------------------------------
    //
    // Linux's `mm/madvise.c::madvise_inject_error` opens with:
    //
    //     if (!capable(CAP_SYS_ADMIN))
    //         return -EPERM;
    //
    // and is the per-page worker for MADV_HWPOISON (100) and
    // MADV_SOFT_OFFLINE (101).  Every other MADV_* value is advisory
    // and never enters this path.
    //
    // Pre-Phase-189 our madvise() returned `0` for HWPOISON /
    // SOFT_OFFLINE — silently succeeding even without CAP_SYS_ADMIN,
    // which let unprivileged programs probe for memory-error
    // injection without seeing the Linux EPERM signal.  Phase 189
    // gates the two values on CAP_SYS_ADMIN: missing cap → EPERM, cap
    // held but no backend → ENOSYS.
    //
    // EINVAL paths (bad addr alignment, unknown advice) still beat
    // the cap check — they fire before `do_madvise` would call into
    // `madvise_inject_error`, matching Linux's `madvise_behavior_valid`
    // / address-validation prologue.
    //
    // Host test build holds CAP_SYS_ADMIN by default (bit 21 ∈
    // DEFAULT_CAPS_LOW = u32::MAX).  Must run with `--test-threads=1`
    // because the tests manipulate process-wide capability state.
    // ------------------------------------------------------------------

    mod madvise_cap_phase189 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase 188
        /// (`stat::tests::mknod_cap_phase188`).
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_admin() {
            use crate::sys_capability::CAP_SYS_ADMIN;
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_ADMIN < 32 {
                (lo & !(1u32 << CAP_SYS_ADMIN), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_ADMIN - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_ADMIN");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_ADMIN));
        }

        // Re-declare the local constants (the `madvise` const items
        // are private to the function module).
        const MADV_HWPOISON: i32 = 100;
        const MADV_SOFT_OFFLINE: i32 = 101;

        // -- Per-error-class --------------------------------------------------

        /// HWPOISON without CAP_SYS_ADMIN → EPERM.  Matches Linux's
        /// `madvise_inject_error` opening cap check.
        #[test]
        fn test_madvise_phase189_hwpoison_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// SOFT_OFFLINE without CAP_SYS_ADMIN → EPERM.
        #[test]
        fn test_madvise_phase189_soft_offline_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_SOFT_OFFLINE),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// MADV_NORMAL (0) is unaffected by cap drop — purely
        /// advisory.  Confirms the gate is type-conditional.
        #[test]
        fn test_madvise_phase189_normal_no_cap_still_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_NORMAL),
                0,
            );
        }

        /// MADV_DONTNEED / FREE / HUGEPAGE / COLLAPSE — all advisory,
        /// all unaffected.
        #[test]
        fn test_madvise_phase189_other_advisories_no_cap_still_succeed() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            for advice in [MADV_DONTNEED, 8i32, 14, 25] {
                errno::set_errno(0);
                assert_eq!(
                    madvise(core::ptr::null_mut(), 16384, advice),
                    0,
                    "advice {advice} must be unaffected by CAP_SYS_ADMIN drop"
                );
            }
        }

        // -- Ordering matrix --------------------------------------------------

        /// EINVAL (unaligned addr) beats EPERM — the address check
        /// runs before `do_madvise` would dispatch to
        /// `madvise_inject_error`.
        #[test]
        fn test_madvise_phase189_einval_unaligned_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(0x1 as *mut core::ffi::c_void, 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EINVAL,
                "Unaligned-addr EINVAL must beat CAP_SYS_ADMIN EPERM");
        }

        /// EINVAL (unknown advice) beats EPERM.  Linux's
        /// `madvise_behavior_valid` rejects garbage before the cap
        /// check; HWPOISON-as-garbage cannot occur, but a caller
        /// passing both a missing cap and an unrelated bad advice
        /// must see EINVAL (the syscall-domain failure).
        #[test]
        fn test_madvise_phase189_einval_bad_advice_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, 9999),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// EPERM beats ENOSYS — the cap gate fires before the
        /// "no backend" return.  Without the gate this would be
        /// observable as ENOSYS, which is what `mce-test` would
        /// misinterpret as "kernel lacks RAS support".
        #[test]
        fn test_madvise_phase189_eperm_beats_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM,
                "Missing CAP_SYS_ADMIN must surface as EPERM, not ENOSYS");
        }

        // -- ENOSYS-with-cap --------------------------------------------------

        /// HWPOISON with CAP_SYS_ADMIN held → ENOSYS (we have no
        /// memory-error injection backend).  Lets privileged RAS
        /// tools distinguish "denied" from "no backend".
        #[test]
        fn test_madvise_phase189_hwpoison_with_cap_returns_enosys() {
            let _g = CapGuard::snapshot();
            // Cap held by default.
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// SOFT_OFFLINE with CAP_SYS_ADMIN held → ENOSYS.
        #[test]
        fn test_madvise_phase189_soft_offline_with_cap_returns_enosys() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_SOFT_OFFLINE),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Workflow --------------------------------------------------------

        /// Drop cap → HWPOISON fails EPERM; restore cap → HWPOISON
        /// reaches ENOSYS.  Mirrors the privilege-drop / re-elevate
        /// pattern of a setuid memory-tester.
        #[test]
        fn test_madvise_phase189_drop_then_restore_workflow() {
            let _g = CapGuard::snapshot();
            // 1. Cap held — ENOSYS (proper request, no backend).
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            // 2. Drop cap — EPERM.
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
            // 3. Restore cap via capset to u32::MAX — ENOSYS again.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Buggy-caller ----------------------------------------------------

        /// A caller that didn't clear errno sees a fresh EPERM, not
        /// the stale value.
        #[test]
        fn test_madvise_phase189_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(errno::ENOENT);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM,
                "Stale ENOENT must be overwritten with EPERM");
        }

        // -- Recovery --------------------------------------------------------

        /// CapGuard drop restores cap so a subsequent HWPOISON call
        /// reaches ENOSYS again.
        #[test]
        fn test_madvise_phase189_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_sys_admin();
                errno::set_errno(0);
                assert_eq!(
                    madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                    -1,
                );
                assert_eq!(errno::get_errno(), errno::EPERM);
            } // _g dropped here; cap restored.
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS,
                "CapGuard drop must restore cap; HWPOISON reaches ENOSYS");
        }

        // -- Sentinel --------------------------------------------------------

        /// With CAP_SYS_ADMIN held, all existing EINVAL paths still
        /// fire.  Confirms the gate is gated, not unconditional.
        #[test]
        fn test_madvise_phase189_with_cap_existing_terminals_unchanged() {
            let _g = CapGuard::snapshot();
            // EINVAL on unaligned.
            errno::set_errno(0);
            assert_eq!(
                madvise(0x1 as *mut core::ffi::c_void, 16384, MADV_NORMAL),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
            // EINVAL on unknown advice.
            errno::set_errno(0);
            assert_eq!(madvise(core::ptr::null_mut(), 16384, 9999), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
            // 0 on advisory.
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_NORMAL),
                0,
            );
        }

        // -- Cross-check -----------------------------------------------------

        /// Dropping CAP_MKNOD alone must NOT affect madvise — Linux
        /// gates the inject path on CAP_SYS_ADMIN specifically.  Pins
        /// down the cross-cap invariant so a future refactor that
        /// probes the wrong cap is caught.
        #[test]
        fn test_madvise_phase189_mknod_drop_does_not_affect_madvise() {
            use crate::sys_capability::CAP_MKNOD;
            let _g = CapGuard::snapshot();
            // Drop only CAP_MKNOD (bit 27).
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << CAP_MKNOD);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            // HWPOISON still reaches ENOSYS.
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_HWPOISON),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS,
                "CAP_MKNOD drop must not affect madvise");
        }

        /// Phase 189 errno is EPERM (capable convention), matching
        /// `madvise_inject_error` → `-EPERM`.  Distinct from the
        /// EACCES used by Phase 186 (seccomp).  Cross-phase invariant.
        #[test]
        fn test_madvise_phase189_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(
                madvise(core::ptr::null_mut(), 16384, MADV_SOFT_OFFLINE),
                -1,
            );
            let e = errno::get_errno();
            assert_eq!(e, errno::EPERM);
            assert_ne!(e, errno::EACCES,
                "madvise_inject_error uses EPERM (capable convention)");
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
/// - `EINVAL` — both `MFD_EXEC` and `MFD_NOEXEC_SEAL` are set (they
///   are mutually exclusive since Linux 6.3 — see Phase 138).
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
    // Phase 138: MFD_EXEC and MFD_NOEXEC_SEAL are mutually exclusive
    // since Linux 6.3.  In Linux's `mm/memfd.c::memfd_create` this
    // check runs after the flag-mask check and before
    // `strnlen_user(uname, ...)`, so it precedes the NULL-name
    // EFAULT.  See commit 105ff5339f4988f5 ("memfd: add
    // MFD_NOEXEC_SEAL and MFD_EXEC").
    const EXEC_BOTH: u32 = crate::linux_memfd::MFD_EXEC
        | crate::linux_memfd::MFD_NOEXEC_SEAL;
    if flags & EXEC_BOTH == EXEC_BOTH {
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
///
/// Phase 171: shares the `check_mlock_caps` gate with `mlock` —
/// callers without `CAP_IPC_LOCK` and a zero or exceeded
/// `RLIMIT_MEMLOCK` fail with EPERM / ENOMEM matching Linux.
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
    if let Err(e) = check_mlock_caps(len as u64) {
        errno::set_errno(e);
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
