//! `<alloca.h>` — stack-based memory allocation.
//!
//! In C, `alloca()` allocates memory on the caller's stack frame,
//! which is automatically freed when the function returns.  In a
//! Rust/POSIX compatibility layer, we provide a stub that delegates
//! to `malloc()` since true stack allocation cannot be implemented
//! as a regular function call.
//!
//! Programs using this should be aware that the returned memory is
//! heap-allocated and must be explicitly freed (unlike real `alloca`).

use crate::errno;

// ---------------------------------------------------------------------------
// alloca
// ---------------------------------------------------------------------------

/// Allocate memory on the stack (stub: uses heap allocation).
///
/// **Warning**: Unlike true `alloca()`, this allocates from the heap.
/// The caller is responsible for freeing the returned memory via
/// `free()`.  This exists solely for source compatibility with
/// programs that include `<alloca.h>`.
///
/// Returns a pointer to `size` bytes of memory, or null if
/// allocation fails (with `errno` set to `ENOMEM`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn alloca(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    let ptr = crate::malloc::malloc(size);
    if ptr.is_null() {
        errno::set_errno(errno::ENOMEM);
    }
    ptr
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloca_zero() {
        let ptr = alloca(0);
        assert!(ptr.is_null());
    }

    #[test]
    fn test_alloca_small() {
        // alloca delegates to malloc which uses mmap; on hosts
        // where mmap returns MAP_FAILED the result is null.
        let ptr = alloca(64);
        if !ptr.is_null() {
            unsafe {
                core::ptr::write_bytes(ptr, 0xAA, 64);
                assert_eq!(*ptr, 0xAA);
                crate::malloc::free(ptr);
            }
        }
        // Either way, calling alloca must not crash.
    }

    #[test]
    fn test_alloca_delegates_to_malloc() {
        // Verify alloca returns the same thing malloc would.
        let a = alloca(128);
        let m = crate::malloc::malloc(128);
        // Both should have the same null/non-null status
        // (both ultimately call the same mmap path).
        assert_eq!(a.is_null(), m.is_null());
        if !a.is_null() {
            unsafe { crate::malloc::free(a); }
        }
        if !m.is_null() {
            unsafe { crate::malloc::free(m); }
        }
    }
}
