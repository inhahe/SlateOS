//! `<linux/stddef.h>` — Standard definitions.
//!
//! Provides fundamental constants and macros used throughout
//! the Linux kernel API, equivalent to the kernel's stddef.h.

// ---------------------------------------------------------------------------
// Boolean values (pre-C99/Rust style)
// ---------------------------------------------------------------------------

/// Boolean false.
pub const FALSE: i32 = 0;
/// Boolean true.
pub const TRUE: i32 = 1;

// ---------------------------------------------------------------------------
// NULL pointer constant
// ---------------------------------------------------------------------------

/// Null pointer as usize (for FFI/syscall interfaces).
pub const NULL: usize = 0;

// ---------------------------------------------------------------------------
// Alignment / offset helpers
// ---------------------------------------------------------------------------

/// Calculate the byte offset of a field in a struct.
///
/// This is a compile-time function equivalent to C's `offsetof()`.
/// Only works with `repr(C)` structs.
///
/// # Safety
/// The field name must be valid for the given type.
#[macro_export]
macro_rules! offset_of {
    ($Type:ty, $field:ident) => {{
        // SAFETY: We create a null pointer, not a reference. We only
        // compute a byte offset; we never dereference.
        let base = core::ptr::null::<$Type>();
        // SAFETY: addr_of! on a raw pointer field is safe in const context.
        let field_ptr = unsafe { core::ptr::addr_of!((*base).$field) };
        (field_ptr as usize) - (base as usize)
    }};
}

// ---------------------------------------------------------------------------
// Size constants
// ---------------------------------------------------------------------------

/// Kernel page size (our OS uses 16 KiB pages).
pub const KERNEL_PAGE_SIZE: usize = 16384;

/// Bits per byte.
pub const BITS_PER_BYTE: usize = 8;

/// Bits per long (64-bit platform).
pub const BITS_PER_LONG: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bool_values() {
        assert_eq!(FALSE, 0);
        assert_eq!(TRUE, 1);
        assert_ne!(FALSE, TRUE);
    }

    #[test]
    fn test_null() {
        assert_eq!(NULL, 0);
    }

    #[test]
    fn test_page_size() {
        assert_eq!(KERNEL_PAGE_SIZE, 16384);
        assert!(KERNEL_PAGE_SIZE.is_power_of_two());
    }

    #[test]
    fn test_bits_per_byte() {
        assert_eq!(BITS_PER_BYTE, 8);
    }

    #[test]
    fn test_bits_per_long() {
        assert_eq!(BITS_PER_LONG, 64);
    }

    #[test]
    fn test_offset_of_macro() {
        #[repr(C)]
        struct TestStruct {
            a: u32,
            b: u64,
        }
        assert_eq!(offset_of!(TestStruct, a), 0);
        assert_eq!(offset_of!(TestStruct, b), 8); // aligned to 8
    }
}
