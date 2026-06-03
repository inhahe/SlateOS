//! C assertion support.
//!
//! Provides `__assert_fail` — the function called by the C `assert()`
//! macro when an assertion fails.  This is the glibc/musl convention.

/// Called when a C assert() fails.
///
/// Prints the assertion failure message to stderr and aborts.
///
/// The C `assert(expr)` macro expands to roughly:
/// ```c
/// if (!(expr))
///     __assert_fail("expr", __FILE__, __LINE__, __func__);
/// ```
///
/// # Safety
///
/// All pointer arguments must be valid null-terminated strings or NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __assert_fail(
    assertion: *const u8,
    file: *const u8,
    line: u32,
    function: *const u8,
) -> ! {
    // Write "Assertion failed: EXPR, file FILE, line LINE, function FUNC\n" to stderr.
    let stderr_fd: i32 = 2;

    let prefix = c"Assertion failed: ";
    let _ = crate::file::write(stderr_fd, prefix.as_ptr().cast::<u8>(), 18);

    if !assertion.is_null() {
        let len = unsafe { crate::string::strlen(assertion) };
        let _ = crate::file::write(stderr_fd, assertion, len);
    }

    let file_prefix = c", file ";
    let _ = crate::file::write(stderr_fd, file_prefix.as_ptr().cast::<u8>(), 7);

    if !file.is_null() {
        let len = unsafe { crate::string::strlen(file) };
        let _ = crate::file::write(stderr_fd, file, len);
    }

    let line_prefix = c", line ";
    let _ = crate::file::write(stderr_fd, line_prefix.as_ptr().cast::<u8>(), 7);

    // Format line number.
    let mut line_buf = [0u8; 10];
    let line_len = format_u32(line, &mut line_buf);
    if line_len > 0 {
        let _ = crate::file::write(stderr_fd, line_buf.as_ptr(), line_len);
    }

    if !function.is_null() {
        let func_prefix = c", function ";
        let _ = crate::file::write(stderr_fd, func_prefix.as_ptr().cast::<u8>(), 11);
        let len = unsafe { crate::string::strlen(function) };
        let _ = crate::file::write(stderr_fd, function, len);
    }

    let nl = b'\n';
    let _ = crate::file::write(stderr_fd, &raw const nl, 1);

    crate::unistd::abort();
}

/// Format a u32 into a decimal string.  Returns the number of bytes written.
fn format_u32(mut val: u32, buf: &mut [u8; 10]) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut digits = [0u8; 10];
    let mut len: usize = 0;
    while val > 0 {
        if let Some(slot) = digits.get_mut(len) {
            *slot = b'0'.wrapping_add((val % 10) as u8);
        }
        val /= 10;
        len = len.wrapping_add(1);
    }

    // Reverse into buf.
    let mut i: usize = 0;
    while i < len {
        if let (Some(dst), Some(src)) = (
            buf.get_mut(i),
            digits.get(len.wrapping_sub(1).wrapping_sub(i)),
        ) {
            *dst = *src;
        }
        i = i.wrapping_add(1);
    }
    len
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- format_u32 --

    #[test]
    fn test_format_u32_zero() {
        let mut buf = [0u8; 10];
        let len = format_u32(0, &mut buf);
        assert_eq!(len, 1);
        assert_eq!(buf[0], b'0');
    }

    #[test]
    fn test_format_u32_one() {
        let mut buf = [0u8; 10];
        let len = format_u32(1, &mut buf);
        assert_eq!(len, 1);
        assert_eq!(buf[0], b'1');
    }

    #[test]
    fn test_format_u32_single_digit() {
        for d in 0..=9u32 {
            let mut buf = [0u8; 10];
            let len = format_u32(d, &mut buf);
            assert_eq!(len, 1);
            assert_eq!(buf[0], b'0' + d as u8);
        }
    }

    #[test]
    fn test_format_u32_two_digits() {
        let mut buf = [0u8; 10];
        let len = format_u32(42, &mut buf);
        assert_eq!(len, 2);
        assert_eq!(&buf[..2], b"42");
    }

    #[test]
    fn test_format_u32_three_digits() {
        let mut buf = [0u8; 10];
        let len = format_u32(123, &mut buf);
        assert_eq!(len, 3);
        assert_eq!(&buf[..3], b"123");
    }

    #[test]
    fn test_format_u32_large() {
        let mut buf = [0u8; 10];
        let len = format_u32(999_999_999, &mut buf);
        assert_eq!(len, 9);
        assert_eq!(&buf[..9], b"999999999");
    }

    #[test]
    fn test_format_u32_max() {
        // u32::MAX = 4294967295 (10 digits — exactly fills the buffer)
        let mut buf = [0u8; 10];
        let len = format_u32(u32::MAX, &mut buf);
        assert_eq!(len, 10);
        assert_eq!(&buf[..10], b"4294967295");
    }

    #[test]
    fn test_format_u32_powers_of_ten() {
        let cases: &[(u32, &[u8])] = &[
            (10, b"10"),
            (100, b"100"),
            (1000, b"1000"),
            (10_000, b"10000"),
            (100_000, b"100000"),
            (1_000_000, b"1000000"),
        ];
        for &(val, expected) in cases {
            let mut buf = [0u8; 10];
            let len = format_u32(val, &mut buf);
            assert_eq!(len, expected.len(), "format_u32({val})");
            assert_eq!(&buf[..len], expected, "format_u32({val})");
        }
    }

    #[test]
    fn test_format_u32_typical_line_numbers() {
        // Line numbers are the primary use case for format_u32.
        let cases: &[(u32, &[u8])] = &[
            (1, b"1"),
            (25, b"25"),
            (256, b"256"),
            (1024, b"1024"),
            (65535, b"65535"),
        ];
        for &(val, expected) in cases {
            let mut buf = [0u8; 10];
            let len = format_u32(val, &mut buf);
            assert_eq!(&buf[..len], expected);
        }
    }
}
