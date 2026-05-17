//! `<execinfo.h>` — backtrace support.
//!
//! Implements `backtrace`, `backtrace_symbols`, `backtrace_symbols_fd`.
//!
//! ## Limitations
//!
//! Stack unwinding is not yet implemented.  `backtrace()` returns 0
//! (no frames captured), `backtrace_symbols()` returns null, and
//! `backtrace_symbols_fd()` is a no-op.
//!
//! These stubs satisfy link-time references from programs (like
//! sanitizers, crash handlers, and debugging libraries) that call
//! the glibc/BSD backtrace API.

// ---------------------------------------------------------------------------
// backtrace
// ---------------------------------------------------------------------------

/// `backtrace` — capture a stack backtrace.
///
/// Stores return addresses from the call stack into `buffer`, up to
/// `size` entries.  Returns the number of addresses captured.
///
/// Stub: always returns 0 (no frames captured).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn backtrace(
    _buffer: *mut *mut u8,
    _size: i32,
) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// backtrace_symbols
// ---------------------------------------------------------------------------

/// `backtrace_symbols` — translate addresses into symbol strings.
///
/// Takes an array of `size` addresses (from `backtrace()`) and returns
/// a malloc'd array of strings describing each address.  The caller
/// must free the returned array (but not the individual strings).
///
/// Stub: always returns null.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn backtrace_symbols(
    _buffer: *const *mut u8,
    _size: i32,
) -> *mut *mut u8 {
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// backtrace_symbols_fd
// ---------------------------------------------------------------------------

/// `backtrace_symbols_fd` — write symbol descriptions to a file descriptor.
///
/// Like `backtrace_symbols`, but writes the strings directly to `fd`
/// instead of returning them.  Does not call malloc.
///
/// Stub: no-op (writes nothing).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn backtrace_symbols_fd(
    _buffer: *const *mut u8,
    _size: i32,
    _fd: i32,
) {
    // No-op: nothing to write.
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // backtrace
    // -----------------------------------------------------------------------

    #[test]
    fn test_backtrace_returns_zero() {
        let mut buf = [core::ptr::null_mut(); 64];
        let ret = backtrace(buf.as_mut_ptr(), 64);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_backtrace_null_buffer() {
        let ret = backtrace(core::ptr::null_mut(), 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_backtrace_zero_size() {
        let mut buf = [core::ptr::null_mut(); 1];
        let ret = backtrace(buf.as_mut_ptr(), 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_backtrace_negative_size() {
        let ret = backtrace(core::ptr::null_mut(), -1);
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // backtrace_symbols
    // -----------------------------------------------------------------------

    #[test]
    fn test_backtrace_symbols_returns_null() {
        let buf = [core::ptr::null_mut(); 10];
        let ret = backtrace_symbols(buf.as_ptr(), 10);
        assert!(ret.is_null());
    }

    #[test]
    fn test_backtrace_symbols_null_buffer() {
        let ret = backtrace_symbols(core::ptr::null(), 0);
        assert!(ret.is_null());
    }

    #[test]
    fn test_backtrace_symbols_zero_size() {
        let ret = backtrace_symbols(core::ptr::null(), 0);
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // backtrace_symbols_fd
    // -----------------------------------------------------------------------

    #[test]
    fn test_backtrace_symbols_fd_no_crash() {
        let buf = [core::ptr::null_mut(); 5];
        backtrace_symbols_fd(buf.as_ptr(), 5, 2);
        // Survived — no crash.
    }

    #[test]
    fn test_backtrace_symbols_fd_null_buffer() {
        backtrace_symbols_fd(core::ptr::null(), 0, 2);
    }

    #[test]
    fn test_backtrace_symbols_fd_invalid_fd() {
        let buf = [core::ptr::null_mut(); 1];
        backtrace_symbols_fd(buf.as_ptr(), 1, -1);
    }

    // -----------------------------------------------------------------------
    // Full workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_capture_and_symbolize_workflow() {
        // Typical usage: capture → symbolize → print.
        let mut addrs = [core::ptr::null_mut(); 128];
        let nframes = backtrace(addrs.as_mut_ptr(), 128);
        assert_eq!(nframes, 0); // stub returns 0

        // backtrace_symbols with 0 frames.
        let symbols = backtrace_symbols(addrs.as_ptr(), nframes);
        assert!(symbols.is_null());

        // backtrace_symbols_fd with 0 frames.
        backtrace_symbols_fd(addrs.as_ptr(), nframes, 2);
    }
}
