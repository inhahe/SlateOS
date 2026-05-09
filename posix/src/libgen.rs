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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
