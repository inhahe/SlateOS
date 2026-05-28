//! C dynamic memory allocation.
//!
//! Implements `malloc`, `free`, `calloc`, `realloc`, `posix_memalign`,
//! `aligned_alloc`, `valloc`, `pvalloc`, `memalign`, `malloc_usable_size`
//! using mmap/munmap as the backing allocator.
//!
//! ## Design
//!
//! This is a simple allocator that prepends a header to each allocation
//! recording the mmap base address and size (so `free` and `realloc`
//! know what to unmap).  Every allocation is its own mmap region.  This
//! is correct but not efficient — a real allocator (dlmalloc, jemalloc)
//! would batch small allocations into larger arenas.  This is
//! intentionally simple because:
//!
//! - Correctness matters more than performance at this stage
//! - Programs needing a real allocator can link one in later
//! - It exercises the mmap/munmap syscall path
//!
//! ## Header Layout
//!
//! ```text
//! +-----------+-----------+------------------+
//! | base: u64 | size: u64 | user data...     |
//! +-----------+-----------+------------------+
//! ^                        ^
//! (may differ for aligned)  returned pointer
//! ```
//!
//! For standard malloc, the header is at `mmap_base` and the user
//! pointer is at `mmap_base + 16`.  For aligned allocations with
//! alignment > 16, the header is placed just before the aligned user
//! pointer — `base` stores the actual mmap start address so `free()`
//! can always unmap the correct region regardless of pointer alignment.
//!
//! The header is 16 bytes (aligned to 16 for ABI compliance).

use crate::mman;

/// Header size: 16 bytes (u64 mmap_base + u64 total_size).
/// Placed immediately before the user pointer.
const HEADER_SIZE: usize = 16;

/// Allocate `size` bytes of memory.
///
/// Returns a pointer to at least `size` bytes of memory, or NULL
/// on failure.  The memory is not initialized.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
    if size == 0 {
        // POSIX: malloc(0) may return NULL or a unique pointer.
        // We return NULL for simplicity.
        return core::ptr::null_mut();
    }

    let Some(total) = size.checked_add(HEADER_SIZE) else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    };

    let ptr = mman::mmap(
        core::ptr::null_mut(),
        total,
        mman::PROT_READ | mman::PROT_WRITE,
        mman::MAP_PRIVATE | mman::MAP_ANONYMOUS,
        -1,
        0,
    );

    if ptr == mman::MAP_FAILED {
        return core::ptr::null_mut();
    }

    // Write the header: [mmap_base_addr, total_region_size].
    // SAFETY: mmap returned valid memory of at least `total` bytes.
    let base = ptr.cast::<u8>();
    unsafe {
        // Store the mmap base address (= ptr itself for standard malloc).
        core::ptr::write_unaligned(base.cast::<u64>(), base as u64);
        // Store the total mmap region size.
        core::ptr::write_unaligned(base.add(8).cast::<u64>(), total as u64);
    }

    // Return pointer past the header.
    // SAFETY: total >= HEADER_SIZE (checked_add above), so base + 16
    // is within the mapped region.
    unsafe { base.add(HEADER_SIZE) }
}

/// Allocate and zero-initialize memory for `nmemb` elements of `size` bytes.
///
/// Returns NULL on overflow or allocation failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn calloc(nmemb: usize, size: usize) -> *mut u8 {
    let Some(total_size) = nmemb.checked_mul(size) else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    };

    let ptr = malloc(total_size);
    if !ptr.is_null() {
        // mmap returns zeroed memory, so calloc needs no explicit memset.
        // If we ever switch to an arena allocator, this will need to zero.
    }
    ptr
}

/// Change the size of an allocated block.
///
/// If `ptr` is NULL, equivalent to `malloc(size)`.
/// If `size` is 0, equivalent to `free(ptr)` and returns NULL.
///
/// # Safety
///
/// `ptr` must be NULL or a value previously returned by `malloc`,
/// `calloc`, or `realloc` that has not been freed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return malloc(size);
    }

    if size == 0 {
        unsafe { free(ptr); }
        return core::ptr::null_mut();
    }

    // Read the header: [mmap_base, total_mmap_size] at ptr - HEADER_SIZE.
    // SAFETY: ptr was returned by malloc/aligned_alloc, so the header is valid.
    let header = unsafe { ptr.sub(HEADER_SIZE) };
    let mmap_base = unsafe { core::ptr::read_unaligned(header.cast::<u64>()) } as usize;
    let mmap_total = unsafe { core::ptr::read_unaligned(header.add(8).cast::<u64>()) } as usize;
    // Compute usable bytes: from user pointer to end of mapped region.
    // This works correctly for both standard malloc (base + total - ptr = size)
    // and aligned allocations (base + total - aligned_ptr = remaining bytes).
    let old_payload = (mmap_base.wrapping_add(mmap_total)).saturating_sub(ptr as usize);

    // If the existing region is already big enough, keep it.
    if size <= old_payload {
        return ptr;
    }

    // Allocate new block and copy.
    let new_ptr = malloc(size);
    if new_ptr.is_null() {
        return core::ptr::null_mut();
    }

    // Copy the smaller of old and new sizes.
    let copy_size = if old_payload < size { old_payload } else { size };
    // SAFETY: Both pointers are valid for copy_size bytes and do not overlap
    // (new_ptr is a fresh mmap).
    unsafe { crate::string::memcpy(new_ptr, ptr, copy_size); }

    // Free old block.
    unsafe { free(ptr); }

    new_ptr
}

/// Free a previously allocated block.
///
/// If `ptr` is NULL, no operation is performed.
///
/// # Safety
///
/// `ptr` must be NULL or a value previously returned by `malloc`,
/// `calloc`, or `realloc` that has not been freed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }

    // Read header: [mmap_base, total_size] at ptr - HEADER_SIZE.
    // SAFETY: ptr was returned by malloc/calloc/realloc/aligned_alloc,
    // so the 16 bytes before ptr contain the valid header fields.
    let header = unsafe { ptr.sub(HEADER_SIZE) };
    let mmap_base = unsafe { core::ptr::read_unaligned(header.cast::<u64>()) } as usize;
    let total = unsafe { core::ptr::read_unaligned(header.add(8).cast::<u64>()) } as usize;

    // Unmap the entire region using the stored base address.
    // For standard malloc, mmap_base == header.  For aligned allocations,
    // mmap_base points to the original mmap start (before alignment padding).
    let _ = mman::munmap(mmap_base as *mut core::ffi::c_void, total);
}

/// Query the usable size of an allocated block (GNU extension).
///
/// Returns the number of usable bytes in the allocation pointed to
/// by `ptr` — this may be larger than the size originally requested
/// because our allocator rounds up to page-size mmap regions.
///
/// If `ptr` is NULL, returns 0.
///
/// # Safety
///
/// `ptr` must be NULL or a value returned by `malloc`/`calloc`/`realloc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn malloc_usable_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }

    // Read header: [mmap_base, total_size] at ptr - HEADER_SIZE.
    // SAFETY: ptr was returned by malloc, header is valid.
    let header = unsafe { ptr.sub(HEADER_SIZE) };
    let mmap_base = unsafe { core::ptr::read_unaligned(header.cast::<u64>()) } as usize;
    let total = unsafe { core::ptr::read_unaligned(header.add(8).cast::<u64>()) } as usize;

    // Usable bytes = from user pointer to end of mmap region.
    (mmap_base.wrapping_add(total)).saturating_sub(ptr as usize)
}

// ---------------------------------------------------------------------------
// Aligned allocation
// ---------------------------------------------------------------------------

/// Allocate memory aligned to `alignment` bytes.
///
/// POSIX `posix_memalign`: stores the allocated pointer in `*memptr`.
/// Returns 0 on success, or an error code (EINVAL/ENOMEM) — does NOT
/// set errno (per POSIX spec, the error code is the return value).
///
/// `alignment` must be a power of two and a multiple of `sizeof(void *)`.
///
/// # Safety
///
/// `memptr` must be a valid, writable pointer to `*mut u8`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_memalign(memptr: *mut *mut u8, alignment: usize, size: usize) -> i32 {
    if memptr.is_null() {
        return crate::errno::EFAULT;
    }

    // Alignment must be power of two and >= sizeof(void*).
    if alignment < core::mem::size_of::<usize>() || !alignment.is_power_of_two() {
        return crate::errno::EINVAL;
    }

    if size == 0 {
        // SAFETY: memptr verified non-null.
        unsafe { *memptr = core::ptr::null_mut(); }
        return 0;
    }

    // Our malloc always returns mmap'd memory which is page-aligned
    // (typically 4096 or 16384 byte alignment).  Any alignment <=
    // page size is automatically satisfied.  For larger alignments
    // we'd need a custom mmap, but that's exceedingly rare.
    let ptr = malloc(size);
    if ptr.is_null() {
        return crate::errno::ENOMEM;
    }

    // Verify alignment (always true for mmap-backed malloc where
    // HEADER_SIZE=16 and mmap returns page-aligned addresses, so
    // the user pointer is at page_start + 16, which is 16-byte aligned).
    if !(ptr as usize).is_multiple_of(alignment) {
        // If somehow misaligned, fall back to over-allocating.
        // SAFETY: ptr was returned by malloc.
        unsafe { free(ptr); }
        let ptr2 = aligned_alloc_impl(alignment, size);
        if ptr2.is_null() {
            return crate::errno::ENOMEM;
        }
        unsafe { *memptr = ptr2; }
        return 0;
    }

    // SAFETY: memptr verified non-null.
    unsafe { *memptr = ptr; }
    0
}

/// Allocate memory with specified alignment (C11 `aligned_alloc`).
///
/// `alignment` must be a power of two.  `size` must be a multiple of
/// `alignment` (per C11 spec, though many implementations don't enforce
/// this).
///
/// Returns a pointer to aligned memory, or NULL on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut u8 {
    if !alignment.is_power_of_two() || alignment == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }
    aligned_alloc_impl(alignment, size)
}

/// Allocate page-aligned memory (obsolete but still used).
///
/// The returned pointer can be passed to `free()`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn valloc(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    // Page-aligned allocation.  Our OS uses 16 KiB (16384-byte) pages.
    // Delegate to aligned_alloc_impl so the returned pointer has a valid
    // header that free() can use.  The previous implementation returned
    // a raw mmap pointer with no header, which caused memory corruption
    // when free() tried to read the nonexistent header.
    aligned_alloc_impl(16384, size)
}

/// Allocate aligned memory (obsolete but still used by some programs).
///
/// `alignment` must be a power of two.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn memalign(alignment: usize, size: usize) -> *mut u8 {
    aligned_alloc(alignment, size)
}

/// Overflow-safe array allocation.
///
/// Allocates `nmemb * size` bytes, returning null with `ENOMEM` if the
/// multiplication would overflow.  This is safer than `malloc(n * s)`
/// where the multiplication can silently wrap.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn reallocarray(
    ptr: *mut u8,
    nmemb: usize,
    size: usize,
) -> *mut u8 {
    let Some(total) = nmemb.checked_mul(size) else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    };
    // SAFETY: realloc handles null/non-null ptr correctly; total was
    // validated against overflow above.
    unsafe { realloc(ptr, total) }
}

/// Internal aligned allocation implementation.
///
/// Allocates via mmap with extra space for alignment padding, then places
/// the standard `[mmap_base, total_size]` header just before the aligned
/// user pointer.  This means `free()` works uniformly — it always reads
/// the header at `ptr - 16` regardless of alignment.
fn aligned_alloc_impl(alignment: usize, size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    // Our malloc returns 16-byte-aligned pointers.  If alignment <= 16,
    // plain malloc suffices.
    if alignment <= HEADER_SIZE {
        return malloc(size);
    }

    // Allocate via mmap directly with extra space for alignment padding
    // and the 16-byte header.  Worst case: (alignment - 1) bytes of
    // padding plus HEADER_SIZE for the header before the aligned address.
    let Some(total) = size
        .checked_add(alignment.wrapping_sub(1))
        .and_then(|v| v.checked_add(HEADER_SIZE))
    else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    };

    let ptr = mman::mmap(
        core::ptr::null_mut(),
        total,
        mman::PROT_READ | mman::PROT_WRITE,
        mman::MAP_PRIVATE | mman::MAP_ANONYMOUS,
        -1,
        0,
    );

    if ptr == mman::MAP_FAILED {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    }

    let base = ptr as usize;
    // The user pointer must be aligned AND have HEADER_SIZE bytes before
    // it for the [mmap_base, total_size] header.
    let min_user = base.wrapping_add(HEADER_SIZE);
    let aligned_user = (min_user.wrapping_add(alignment.wrapping_sub(1)))
        & !alignment.wrapping_sub(1);

    // Write the standard header at aligned_user - HEADER_SIZE.
    // SAFETY: aligned_user >= base + HEADER_SIZE (by construction of min_user),
    // so the header write is within the mmap'd region.  aligned_user + size
    // <= base + total by the checked_add above.
    unsafe {
        let header = aligned_user.wrapping_sub(HEADER_SIZE) as *mut u8;
        core::ptr::write_unaligned(header.cast::<u64>(), base as u64);
        core::ptr::write_unaligned(header.add(8).cast::<u64>(), total as u64);
    }

    aligned_user as *mut u8
}

/// Allocate page-aligned memory rounded up to a page-size multiple.
///
/// Like `valloc`, but rounds `size` up to the next multiple of the
/// system page size before allocating.  This ensures the entire
/// returned region consists of whole pages.
///
/// The returned pointer can be passed to `free()`.
///
/// This is a GNU/BSD extension — not standardised by POSIX.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pvalloc(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    const PAGE_SIZE: usize = 16384;
    // Round up to the next page-size multiple.
    let rounded = match size.checked_add(PAGE_SIZE.wrapping_sub(1)) {
        Some(v) => v & !PAGE_SIZE.wrapping_sub(1),
        None => {
            crate::errno::set_errno(crate::errno::ENOMEM);
            return core::ptr::null_mut();
        }
    };
    aligned_alloc_impl(PAGE_SIZE, rounded)
}

// ---------------------------------------------------------------------------
// glibc internal aliases
// ---------------------------------------------------------------------------
//
// Some programs call glibc's internal __libc_* symbols directly
// (e.g., when overriding malloc).  These just delegate to our
// implementations.

/// glibc internal: `__libc_malloc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_malloc(size: usize) -> *mut u8 {
    malloc(size)
}

/// glibc internal: `__libc_free`.
///
/// # Safety
///
/// Same requirements as `free`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __libc_free(ptr: *mut u8) {
    unsafe { free(ptr); }
}

/// glibc internal: `__libc_realloc`.
///
/// # Safety
///
/// Same requirements as `realloc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __libc_realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    unsafe { realloc(ptr, size) }
}

/// glibc internal: `__libc_calloc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_calloc(nmemb: usize, size: usize) -> *mut u8 {
    calloc(nmemb, size)
}

/// glibc internal: `__libc_memalign`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_memalign(alignment: usize, size: usize) -> *mut u8 {
    memalign(alignment, size)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // malloc boundary cases
    // -----------------------------------------------------------------------

    #[test]
    fn malloc_zero_returns_null() {
        // POSIX: malloc(0) may return NULL.
        let ptr = malloc(0);
        assert!(ptr.is_null(), "malloc(0) should return NULL");
    }

    #[test]
    fn malloc_overflow_returns_null() {
        // size + HEADER_SIZE overflows → NULL.
        let ptr = malloc(usize::MAX);
        assert!(ptr.is_null(), "malloc(usize::MAX) should return NULL");
    }

    #[test]
    fn malloc_near_overflow_returns_null() {
        // size + 16 would overflow.
        let ptr = malloc(usize::MAX - 8);
        assert!(ptr.is_null(), "malloc(MAX - 8) should return NULL");
    }

    // -----------------------------------------------------------------------
    // calloc boundary cases
    // -----------------------------------------------------------------------

    #[test]
    fn calloc_zero_nmemb() {
        let ptr = calloc(0, 100);
        assert!(ptr.is_null(), "calloc(0, 100) should return NULL");
    }

    #[test]
    fn calloc_zero_size() {
        let ptr = calloc(100, 0);
        assert!(ptr.is_null(), "calloc(100, 0) should return NULL");
    }

    #[test]
    fn calloc_overflow_returns_null() {
        // nmemb * size overflows.
        let ptr = calloc(usize::MAX, 2);
        assert!(ptr.is_null(), "calloc(MAX, 2) should return NULL");
    }

    #[test]
    fn calloc_large_overflow() {
        // Just below MAX for each, product overflows.
        let ptr = calloc(usize::MAX / 2 + 1, 3);
        assert!(ptr.is_null(), "calloc with overflow should return NULL");
    }

    // -----------------------------------------------------------------------
    // free(NULL)
    // -----------------------------------------------------------------------

    #[test]
    fn free_null_is_noop() {
        // Must not crash.
        unsafe { free(core::ptr::null_mut()); }
    }

    // -----------------------------------------------------------------------
    // realloc boundary cases
    // -----------------------------------------------------------------------

    #[test]
    fn realloc_null_is_malloc() {
        // realloc(NULL, size) should behave like malloc(size).
        // We test the degenerate case: realloc(NULL, 0) = malloc(0) = NULL.
        let ptr = unsafe { realloc(core::ptr::null_mut(), 0) };
        assert!(ptr.is_null(), "realloc(NULL, 0) should return NULL");
    }

    // -----------------------------------------------------------------------
    // posix_memalign validation
    // -----------------------------------------------------------------------

    #[test]
    fn posix_memalign_null_memptr() {
        let ret = posix_memalign(core::ptr::null_mut(), 16, 100);
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn posix_memalign_alignment_not_power_of_two() {
        let mut ptr: *mut u8 = core::ptr::null_mut();
        let ret = posix_memalign(&raw mut ptr, 3, 100);
        assert_eq!(ret, crate::errno::EINVAL);
        assert!(ptr.is_null());
    }

    #[test]
    fn posix_memalign_alignment_too_small() {
        // Alignment must be >= sizeof(void*) = 8 on x86_64.
        let mut ptr: *mut u8 = core::ptr::null_mut();
        let ret = posix_memalign(&raw mut ptr, 4, 100);
        assert_eq!(ret, crate::errno::EINVAL);
        assert!(ptr.is_null());
    }

    #[test]
    fn posix_memalign_zero_size() {
        // POSIX: posix_memalign with size=0 stores NULL in *memptr.
        let mut ptr: *mut u8 = 0x1234_usize as *mut u8; // garbage
        let ret = posix_memalign(&raw mut ptr, 8, 0);
        assert_eq!(ret, 0);
        assert!(ptr.is_null());
    }

    #[test]
    fn posix_memalign_alignment_one() {
        // alignment=1 is a power of two but < sizeof(void*).
        let mut ptr: *mut u8 = core::ptr::null_mut();
        let ret = posix_memalign(&raw mut ptr, 1, 100);
        assert_eq!(ret, crate::errno::EINVAL);
    }

    #[test]
    fn posix_memalign_alignment_two() {
        // alignment=2 is a power of two but < sizeof(void*) on 64-bit.
        let mut ptr: *mut u8 = core::ptr::null_mut();
        let ret = posix_memalign(&raw mut ptr, 2, 100);
        assert_eq!(ret, crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // aligned_alloc validation
    // -----------------------------------------------------------------------

    #[test]
    fn aligned_alloc_zero_alignment() {
        let ptr = aligned_alloc(0, 100);
        assert!(ptr.is_null(), "aligned_alloc(0, 100) should fail");
    }

    #[test]
    fn aligned_alloc_non_power_of_two() {
        let ptr = aligned_alloc(3, 100);
        assert!(ptr.is_null(), "aligned_alloc(3, 100) should fail");

        let ptr = aligned_alloc(6, 100);
        assert!(ptr.is_null(), "aligned_alloc(6, 100) should fail");
    }

    #[test]
    fn aligned_alloc_zero_size() {
        // aligned_alloc with size=0: our impl returns NULL.
        let ptr = aligned_alloc(16, 0);
        assert!(ptr.is_null(), "aligned_alloc(16, 0) should return NULL");
    }

    // -----------------------------------------------------------------------
    // valloc
    // -----------------------------------------------------------------------

    #[test]
    fn valloc_zero_size() {
        let ptr = valloc(0);
        assert!(ptr.is_null(), "valloc(0) should return NULL");
    }

    // -----------------------------------------------------------------------
    // reallocarray overflow
    // -----------------------------------------------------------------------

    #[test]
    fn reallocarray_overflow() {
        let ptr = reallocarray(core::ptr::null_mut(), usize::MAX, 2);
        assert!(ptr.is_null(), "reallocarray overflow should return NULL");
    }

    #[test]
    fn reallocarray_zero() {
        // reallocarray(NULL, 0, 100) = realloc(NULL, 0) = malloc(0) = NULL.
        let ptr = reallocarray(core::ptr::null_mut(), 0, 100);
        assert!(ptr.is_null(), "reallocarray(NULL, 0, 100) → NULL");
    }

    // -----------------------------------------------------------------------
    // malloc_usable_size
    // -----------------------------------------------------------------------

    #[test]
    fn malloc_usable_size_null() {
        let size = unsafe { malloc_usable_size(core::ptr::null_mut()) };
        assert_eq!(size, 0, "malloc_usable_size(NULL) should be 0");
    }

    // -----------------------------------------------------------------------
    // Header size constant
    // -----------------------------------------------------------------------

    #[test]
    fn header_size_is_16() {
        // Must be 16 for ABI compliance (16-byte aligned user pointers).
        assert_eq!(HEADER_SIZE, 16);
    }

    // -----------------------------------------------------------------------
    // pvalloc
    // -----------------------------------------------------------------------

    #[test]
    fn pvalloc_zero_size() {
        let ptr = pvalloc(0);
        assert!(ptr.is_null(), "pvalloc(0) should return NULL");
    }

    #[test]
    fn pvalloc_overflow() {
        // size close to usize::MAX should fail gracefully.
        let ptr = pvalloc(usize::MAX);
        assert!(ptr.is_null(), "pvalloc(MAX) should return NULL");
    }

    #[test]
    fn pvalloc_near_overflow() {
        // size + PAGE_SIZE - 1 would overflow.
        let ptr = pvalloc(usize::MAX - 100);
        assert!(ptr.is_null(), "pvalloc(MAX-100) should return NULL");
    }

    // -----------------------------------------------------------------------
    // memalign validation
    // -----------------------------------------------------------------------

    #[test]
    fn memalign_zero_alignment() {
        let ptr = memalign(0, 100);
        assert!(ptr.is_null(), "memalign(0, 100) should fail");
    }

    #[test]
    fn memalign_non_power_of_two() {
        let ptr = memalign(3, 100);
        assert!(ptr.is_null(), "memalign(3, 100) should fail");
    }

    #[test]
    fn memalign_zero_size() {
        let ptr = memalign(16, 0);
        assert!(ptr.is_null(), "memalign(16, 0) should return NULL");
    }

    // -----------------------------------------------------------------------
    // valloc overflow
    // -----------------------------------------------------------------------

    #[test]
    fn valloc_overflow() {
        let ptr = valloc(usize::MAX);
        assert!(ptr.is_null(), "valloc(MAX) should return NULL");
    }

    // -----------------------------------------------------------------------
    // reallocarray valid parameters (without actual mmap)
    // -----------------------------------------------------------------------

    #[test]
    fn reallocarray_null_zero_zero() {
        let ptr = reallocarray(core::ptr::null_mut(), 0, 0);
        assert!(ptr.is_null());
    }

    #[test]
    fn reallocarray_one_one_null() {
        // realloc(NULL, 1) would try mmap, which returns MAP_FAILED in test.
        // Just verify it doesn't crash.
        let ptr = reallocarray(core::ptr::null_mut(), 1, 1);
        // mmap fails in test mode, so this should be null
        assert!(ptr.is_null());
    }

    // -----------------------------------------------------------------------
    // posix_memalign: valid alignments with zero size
    // -----------------------------------------------------------------------

    #[test]
    fn posix_memalign_valid_alignments_zero_size() {
        // All power-of-two alignments >= 8 should succeed with size=0
        for shift in 3..=20u32 {
            let align = 1usize << shift;
            let mut ptr: *mut u8 = 0x1234_usize as *mut u8;
            let ret = posix_memalign(&raw mut ptr, align, 0);
            assert_eq!(ret, 0, "posix_memalign(_, {align}, 0) should succeed");
            assert!(ptr.is_null(), "size=0 should store NULL");
        }
    }

    // -----------------------------------------------------------------------
    // calloc: both zero
    // -----------------------------------------------------------------------

    #[test]
    fn calloc_both_zero() {
        let ptr = calloc(0, 0);
        assert!(ptr.is_null());
    }

    // -----------------------------------------------------------------------
    // malloc: small sizes don't crash (they just fail because no mmap)
    // -----------------------------------------------------------------------

    #[test]
    fn malloc_small_returns_null_in_test() {
        // mmap is not available in test mode, so malloc should return NULL
        let ptr = malloc(1);
        assert!(ptr.is_null());
    }

    #[test]
    fn malloc_page_size_returns_null_in_test() {
        let ptr = malloc(16384);
        assert!(ptr.is_null());
    }

    // -----------------------------------------------------------------------
    // realloc: overflow size returns NULL
    // -----------------------------------------------------------------------

    #[test]
    fn realloc_null_overflow_size() {
        let ptr = unsafe { realloc(core::ptr::null_mut(), usize::MAX) };
        assert!(ptr.is_null());
    }

    // -----------------------------------------------------------------------
    // aligned_alloc: valid powers of two with zero size
    // -----------------------------------------------------------------------

    #[test]
    fn aligned_alloc_valid_align_zero_size() {
        for shift in 0..=16u32 {
            let align = 1usize << shift;
            let ptr = aligned_alloc(align, 0);
            assert!(ptr.is_null(), "aligned_alloc({align}, 0) should return NULL");
        }
    }

    // -----------------------------------------------------------------------
    // glibc alias behavior (if functions exist)
    // -----------------------------------------------------------------------

    #[test]
    fn libc_malloc_zero() {
        let ptr = __libc_malloc(0);
        assert!(ptr.is_null());
    }

    #[test]
    fn libc_free_null() {
        unsafe { __libc_free(core::ptr::null_mut()); }
    }

    #[test]
    fn libc_realloc_null_zero() {
        let ptr = unsafe { __libc_realloc(core::ptr::null_mut(), 0) };
        assert!(ptr.is_null());
    }

    #[test]
    fn libc_calloc_zero() {
        let ptr = __libc_calloc(0, 0);
        assert!(ptr.is_null());
    }

    #[test]
    fn libc_memalign_zero_size() {
        let ptr = __libc_memalign(16, 0);
        assert!(ptr.is_null());
    }
}
