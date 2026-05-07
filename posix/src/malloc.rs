//! C dynamic memory allocation.
//!
//! Implements `malloc`, `free`, `calloc`, `realloc` using mmap/munmap
//! as the backing allocator.
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
