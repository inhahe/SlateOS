//! POSIX `<libgen.h>` — path component extraction.
//!
//! Implements `basename()` and `dirname()` per POSIX.1-2024.
//!
//! ## POSIX Semantics
//!
//! These functions may modify the input string and return pointers
//! into it (or to static storage for special cases).  The caller
//! must not pass string literals and must copy the result before
//! calling the function again.
//!
//! ## Limitations
//!
//! - Uses internal static buffers for the "." and "/" results,
//!   which makes these functions non-reentrant (matching POSIX spec).

/// Static buffer for returning "." when needed.
static DOT: [u8; 2] = [b'.', 0];
/// Static buffer for returning "/" when needed.
static SLASH: [u8; 2] = [b'/', 0];

/// Return the last component of a pathname.
///
/// POSIX rules:
/// - If `path` is null or empty, returns ".".
/// - Trailing '/' characters are removed.
/// - If the path is all '/' characters, returns "/".
/// - Otherwise, returns the portion after the last '/'.
///
/// The returned pointer may point into the modified `path` string
/// or to a static buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn basename(path: *mut u8) -> *mut u8 {
    if path.is_null() {
        return DOT.as_ptr().cast_mut();
    }

    let len = unsafe { crate::string::strlen(path) };
    if len == 0 {
        return DOT.as_ptr().cast_mut();
    }

    // Strip trailing slashes.
    let mut end = len;
    while end > 0 {
        let prev = end.wrapping_sub(1);
        if unsafe { *path.add(prev) } != b'/' {
            break;
        }
        end = prev;
    }

    // All slashes → return "/".
    if end == 0 {
        return SLASH.as_ptr().cast_mut();
    }

    // Null-terminate after stripped trailing slashes.
    // SAFETY: end <= len, so path.add(end) is within the original buffer.
    unsafe { *path.add(end) = 0; }

    // Find the last slash before `end`.
    let mut last_slash = end;
    while last_slash > 0 {
        let prev = last_slash.wrapping_sub(1);
        if unsafe { *path.add(prev) } == b'/' {
            break;
        }
        last_slash = prev;
    }

    if last_slash == 0 && unsafe { *path } != b'/' {
        // No slash found — the entire string is the basename.
        return path;
    }

    // Return the portion after the last slash.
    // SAFETY: last_slash < end, so path.add(last_slash) is valid.
    unsafe { path.add(last_slash) }
}

/// Return the directory component of a pathname.
///
/// POSIX rules:
/// - If `path` is null or empty, returns ".".
/// - Trailing '/' characters are removed (from the basename portion).
/// - If no '/' is found, returns ".".
/// - If the path is all '/' characters, returns "/".
/// - Trailing '/' characters on the directory are removed.
///
/// The returned pointer may point into the modified `path` string
/// or to a static buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dirname(path: *mut u8) -> *mut u8 {
    if path.is_null() {
        return DOT.as_ptr().cast_mut();
    }

    let len = unsafe { crate::string::strlen(path) };
    if len == 0 {
        return DOT.as_ptr().cast_mut();
    }

    // Strip trailing slashes.
    let mut end = len;
    while end > 0 {
        let prev = end.wrapping_sub(1);
        if unsafe { *path.add(prev) } != b'/' {
            break;
        }
        end = prev;
    }

    // All slashes → return "/".
    if end == 0 {
        return SLASH.as_ptr().cast_mut();
    }

    // Strip the basename (everything after last slash).
    while end > 0 {
        let prev = end.wrapping_sub(1);
        if unsafe { *path.add(prev) } == b'/' {
            break;
        }
        end = prev;
    }

    // No slash found → return ".".
    if end == 0 {
        return DOT.as_ptr().cast_mut();
    }

    // Strip trailing slashes from the directory part.
    while end > 1 {
        let prev = end.wrapping_sub(1);
        if unsafe { *path.add(prev) } != b'/' {
            break;
        }
        end = prev;
    }

    // Null-terminate the directory.
    // SAFETY: end <= len, so path.add(end) is valid.
    unsafe { *path.add(end) = 0; }

    path
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Helper: compare a C string pointer against an expected byte slice.
    /// Returns true if the C string at `ptr` equals `expected` (not including
    /// the null terminator, which must be present at `ptr`).
    unsafe fn cstr_eq(ptr: *const u8, expected: &[u8]) -> bool {
        for (i, &b) in expected.iter().enumerate() {
            // SAFETY: caller guarantees `ptr` points to a valid C string
            // at least `expected.len() + 1` bytes long.
            if unsafe { *ptr.add(i) } != b {
                return false;
            }
        }
        // The byte after expected must be the null terminator.
        // SAFETY: same precondition as above.
        unsafe { *ptr.add(expected.len()) == 0 }
    }

    /// Helper: create a mutable null-terminated byte buffer from a string.
    /// Returns a Vec<u8> that the caller must keep alive for the pointer's
    /// lifetime.
    fn make_path(s: &str) -> Vec<u8> {
        let mut v: Vec<u8> = s.as_bytes().to_vec();
        v.push(0); // null terminator
        v
    }

    // -----------------------------------------------------------------------
    // basename
    // -----------------------------------------------------------------------

    #[test]
    fn test_basename_usr_lib() {
        let mut buf = make_path("/usr/lib");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"lib") });
    }

    #[test]
    fn test_basename_trailing_slash() {
        let mut buf = make_path("/usr/");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"usr") });
    }

    #[test]
    fn test_basename_no_slash() {
        let mut buf = make_path("usr");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"usr") });
    }

    #[test]
    fn test_basename_root() {
        let mut buf = make_path("/");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/") });
    }

    #[test]
    fn test_basename_empty() {
        let mut buf = make_path("");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_basename_null() {
        let result = basename(ptr::null_mut());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_basename_multiple_trailing_slashes() {
        let mut buf = make_path("/usr///");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"usr") });
    }

    #[test]
    fn test_basename_all_slashes() {
        let mut buf = make_path("///");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/") });
    }

    #[test]
    fn test_basename_deep_path() {
        let mut buf = make_path("/a/b/c/d");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"d") });
    }

    #[test]
    fn test_basename_single_char() {
        let mut buf = make_path("x");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"x") });
    }

    #[test]
    fn test_basename_dot() {
        let mut buf = make_path(".");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_basename_dotdot() {
        let mut buf = make_path("..");
        let result = basename(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"..") });
    }

    // -----------------------------------------------------------------------
    // dirname
    // -----------------------------------------------------------------------

    #[test]
    fn test_dirname_usr_lib() {
        let mut buf = make_path("/usr/lib");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/usr") });
    }

    #[test]
    fn test_dirname_trailing_slash() {
        let mut buf = make_path("/usr/");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/") });
    }

    #[test]
    fn test_dirname_no_slash() {
        let mut buf = make_path("usr");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_dirname_root() {
        let mut buf = make_path("/");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/") });
    }

    #[test]
    fn test_dirname_empty() {
        let mut buf = make_path("");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_dirname_null() {
        let result = dirname(ptr::null_mut());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_dirname_multiple_trailing_slashes() {
        let mut buf = make_path("/usr///");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/") });
    }

    #[test]
    fn test_dirname_all_slashes() {
        let mut buf = make_path("///");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/") });
    }

    #[test]
    fn test_dirname_deep_path() {
        let mut buf = make_path("/a/b/c/d");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/a/b/c") });
    }

    #[test]
    fn test_dirname_single_char() {
        let mut buf = make_path("x");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_dirname_file_in_root() {
        let mut buf = make_path("/file");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b"/") });
    }

    #[test]
    fn test_dirname_dot() {
        let mut buf = make_path(".");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_dirname_dotdot() {
        let mut buf = make_path("..");
        let result = dirname(buf.as_mut_ptr());
        assert!(unsafe { cstr_eq(result, b".") });
    }

    #[test]
    fn test_dirname_redundant_slashes_in_middle() {
        let mut buf = make_path("/usr///lib");
        let result = dirname(buf.as_mut_ptr());
        // After stripping basename "lib", the directory part is "/usr///"
        // which has trailing slashes stripped down to "/usr".
        assert!(unsafe { cstr_eq(result, b"/usr") });
    }
}
