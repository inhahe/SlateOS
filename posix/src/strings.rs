//! `<strings.h>` — BSD string operations.
//!
//! Re-exports legacy BSD string functions from the `string` module.
//! Also provides `index`, `rindex`, and `bcmp` which are BSD-origin
//! functions defined by this header.
//!
//! POSIX.1-2024 marks most of these as legacy; prefer the standard
//! `<string.h>` equivalents (`memcmp`, `strchr`, `strrchr`, etc.).

pub use crate::string::bcopy;
pub use crate::string::bzero;
pub use crate::string::explicit_bzero;
pub use crate::string::strcasecmp;
pub use crate::string::strncasecmp;
pub use crate::string::ffs;

// ---------------------------------------------------------------------------
// bcmp — compare byte strings (legacy, equivalent to memcmp != 0)
// ---------------------------------------------------------------------------

/// Compare `n` bytes of `s1` and `s2`.
///
/// Returns 0 if equal, non-zero otherwise.  Unlike `memcmp`, the
/// non-zero return value is not required to indicate ordering.
///
/// # Safety
///
/// Both pointers must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn bcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    // SAFETY: Caller guarantees both pointers are valid for `n` bytes.
    unsafe { crate::string::memcmp(s1, s2, n) }
}

// ---------------------------------------------------------------------------
// index — find first occurrence of a character (legacy strchr)
// ---------------------------------------------------------------------------

/// Locate the first occurrence of `c` in the null-terminated string `s`.
///
/// Equivalent to `strchr(s, c)`.
///
/// # Safety
///
/// `s` must point to a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn index(s: *const u8, c: i32) -> *const u8 {
    // SAFETY: Caller guarantees `s` is a valid C string.
    unsafe { crate::string::strchr(s, c) }
}

// ---------------------------------------------------------------------------
// rindex — find last occurrence of a character (legacy strrchr)
// ---------------------------------------------------------------------------

/// Locate the last occurrence of `c` in the null-terminated string `s`.
///
/// Equivalent to `strrchr(s, c)`.
///
/// # Safety
///
/// `s` must point to a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rindex(s: *const u8, c: i32) -> *const u8 {
    // SAFETY: Caller guarantees `s` is a valid C string.
    unsafe { crate::string::strrchr(s, c) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bcmp_equal() {
        let a = b"hello";
        let b = b"hello";
        assert_eq!(unsafe { bcmp(a.as_ptr(), b.as_ptr(), 5) }, 0);
    }

    #[test]
    fn test_bcmp_different() {
        let a = b"hello";
        let b = b"world";
        assert_ne!(unsafe { bcmp(a.as_ptr(), b.as_ptr(), 5) }, 0);
    }

    #[test]
    fn test_bcmp_zero_length() {
        let a = b"abc";
        let b = b"xyz";
        assert_eq!(unsafe { bcmp(a.as_ptr(), b.as_ptr(), 0) }, 0);
    }

    #[test]
    fn test_index_found() {
        let s = b"hello world\0";
        let p = unsafe { index(s.as_ptr(), b'o' as i32) };
        assert!(!p.is_null());
        let offset = p as usize - s.as_ptr() as usize;
        assert_eq!(offset, 4); // first 'o' at index 4
    }

    #[test]
    fn test_index_not_found() {
        let s = b"hello\0";
        let p = unsafe { index(s.as_ptr(), b'z' as i32) };
        assert!(p.is_null());
    }

    #[test]
    fn test_rindex_found() {
        let s = b"hello world\0";
        let p = unsafe { rindex(s.as_ptr(), b'o' as i32) };
        assert!(!p.is_null());
        let offset = p as usize - s.as_ptr() as usize;
        assert_eq!(offset, 7); // last 'o' at index 7
    }

    #[test]
    fn test_rindex_not_found() {
        let s = b"hello\0";
        let p = unsafe { rindex(s.as_ptr(), b'z' as i32) };
        assert!(p.is_null());
    }

    #[test]
    fn test_ffs_zero() {
        assert_eq!(ffs(0), 0);
    }

    #[test]
    fn test_ffs_one() {
        assert_eq!(ffs(1), 1);
    }

    #[test]
    fn test_ffs_power_of_two() {
        assert_eq!(ffs(8), 4); // bit 4 (1-indexed)
    }

    #[test]
    fn test_strcasecmp_equal() {
        let a = b"Hello\0";
        let b = b"hello\0";
        assert_eq!(unsafe { strcasecmp(a.as_ptr(), b.as_ptr()) }, 0);
    }

    #[test]
    fn test_strcasecmp_different() {
        let a = b"abc\0";
        let b = b"xyz\0";
        assert_ne!(unsafe { strcasecmp(a.as_ptr(), b.as_ptr()) }, 0);
    }

    #[test]
    fn test_strncasecmp_prefix() {
        let a = b"Hello World\0";
        let b = b"hello earth\0";
        assert_eq!(unsafe { strncasecmp(a.as_ptr(), b.as_ptr(), 5) }, 0);
    }

    #[test]
    fn test_cross_module_ffs() {
        assert_eq!(ffs(16), crate::string::ffs(16));
    }

    #[test]
    fn test_cross_module_strcasecmp() {
        let a = b"ABC\0";
        let b = b"abc\0";
        let r1 = unsafe { strcasecmp(a.as_ptr(), b.as_ptr()) };
        let r2 = unsafe { crate::string::strcasecmp(a.as_ptr(), b.as_ptr()) };
        assert_eq!(r1, r2);
    }
}
