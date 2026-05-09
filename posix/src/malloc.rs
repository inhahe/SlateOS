//! C dynamic memory allocation.
//!
//! Implements `malloc`, `free`, `calloc`, `realloc`, `posix_memalign`,
//! `aligned_alloc`, `valloc`, `memalign` using mmap/munmap as the
//! backing allocator.
//!
//! ## Design
//!
//! This is a simple allocator that prepends a header to each allocation
//! recording the size (so `free` and `realloc` know how much to unmap).
//! Every allocation is its own mmap region.  This is correct but not
//! efficient — a real allocator (dlmalloc, jemalloc) would batch small
//! allocations into larger arenas.  This is intentionally simple because:
//!
//! - Correctness matters more than performance at this stage
//! - Programs needing a real allocator can link one in later
//! - It exercises the mmap/munmap syscall path
//!
//! ## Header Layout
//!
//! ```text
//! +----------+------------------+
//! | size: u64 | user data...    |
//! +----------+------------------+
//! ^           ^
//! mmap addr   returned pointer
//! ```
//!
//! The header is 16 bytes (aligned to 16 for ABI compliance).

use crate::mman;

/// Header size, aligned to 16 bytes for ABI compliance.
/// Stores the total mmap region size (header + payload).
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

    // Write the total region size into the header.
    // SAFETY: mmap returned valid memory of at least `total` bytes.
    // mmap always returns page-aligned addresses, so alignment is guaranteed,
    // but we use write_unaligned to satisfy clippy's cast_ptr_alignment.
    unsafe { ptr.cast::<u64>().write_unaligned(total as u64); }

    // Return pointer past the header.
    // SAFETY: ptr is a valid mmap return (at least `total` bytes),
    // and total >= HEADER_SIZE (checked_add above), so ptr + HEADER_SIZE
    // is within the mapped region.
    unsafe { ptr.cast::<u8>().add(HEADER_SIZE) }
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

    // Read the old size from the header.
    // SAFETY: ptr was returned by malloc, so ptr - HEADER_SIZE is the
    // mmap base, which is page-aligned and valid for u64 read.
    let old_total = unsafe { ptr.sub(HEADER_SIZE).cast::<u64>().read_unaligned() } as usize;
    let old_payload = old_total.saturating_sub(HEADER_SIZE);

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

    // Read total size from header.
    let base = unsafe { ptr.sub(HEADER_SIZE) };
    // SAFETY: base was the original mmap return address (page-aligned, valid).
    let total = unsafe { base.cast::<u64>().read_unaligned() } as usize;

    // Unmap the entire region.
    let _ = mman::munmap(base.cast::<core::ffi::c_void>(), total);
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
#[unsafe(no_mangle)]
pub extern "C" fn valloc(size: usize) -> *mut u8 {
    // mmap always returns page-aligned memory; our malloc uses
    // a 16-byte header, so the user pointer is at page+16.
    // For true page alignment, use mmap directly.
    if size == 0 {
        return core::ptr::null_mut();
    }

    let ptr = mman::mmap(
        core::ptr::null_mut(),
        size,
        mman::PROT_READ | mman::PROT_WRITE,
        mman::MAP_PRIVATE | mman::MAP_ANONYMOUS,
        -1,
        0,
    );

    if ptr == mman::MAP_FAILED {
        return core::ptr::null_mut();
    }

    ptr.cast::<u8>()
}

/// Allocate aligned memory (obsolete but still used by some programs).
///
/// `alignment` must be a power of two.
#[unsafe(no_mangle)]
pub extern "C" fn memalign(alignment: usize, size: usize) -> *mut u8 {
    aligned_alloc(alignment, size)
}

/// Internal aligned allocation implementation.
///
/// Over-allocates and adjusts the returned pointer to satisfy alignment.
/// Uses a secondary header to track the real base for free().
fn aligned_alloc_impl(alignment: usize, size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    // Our malloc returns 16-byte-aligned pointers.  If alignment <= 16,
    // plain malloc suffices.
    if alignment <= HEADER_SIZE {
        return malloc(size);
    }

    // Over-allocate: size + alignment + header space for the real base pointer.
    let extra = alignment.wrapping_add(core::mem::size_of::<usize>());
    let Some(total) = size.checked_add(extra) else {
        return core::ptr::null_mut();
    };

    let raw = malloc(total);
    if raw.is_null() {
        return core::ptr::null_mut();
    }

    // Align the user pointer.
    let raw_addr = raw as usize;
    // Reserve space for the back-pointer (one usize before the aligned addr).
    let min_addr = raw_addr.wrapping_add(core::mem::size_of::<usize>());
    let aligned_addr = min_addr.wrapping_add(alignment.wrapping_sub(1)) & !alignment.wrapping_sub(1);

    // Store the original malloc pointer just before the aligned address.
    // SAFETY: aligned_addr - sizeof(usize) >= raw_addr, so this is within
    // the malloc'd region.
    unsafe {
        let back_ptr = (aligned_addr.wrapping_sub(core::mem::size_of::<usize>())) as *mut usize;
        core::ptr::write_unaligned(back_ptr, raw_addr);
    }

    aligned_addr as *mut u8
}
