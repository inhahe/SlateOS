//! C dynamic memory allocation.
//!
//! Implements `malloc`, `free`, `calloc`, `realloc`, `posix_memalign`,
//! `aligned_alloc`, `valloc`, `memalign` using mmap/munmap as the
//! backing allocator.
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn posix_memalign(memptr: *mut *mut u8, alignment: usize, size: usize) -> i32 {
    if memptr.is_null() {
        return crate::errno::EINVAL;
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn memalign(alignment: usize, size: usize) -> *mut u8 {
    aligned_alloc(alignment, size)
}

/// Overflow-safe array allocation.
///
/// Allocates `nmemb * size` bytes, returning null with `ENOMEM` if the
/// multiplication would overflow.  This is safer than `malloc(n * s)`
/// where the multiplication can silently wrap.
#[unsafe(no_mangle)]
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
