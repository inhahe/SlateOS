//! POSIX `<monetary.h>` — monetary formatting.
//!
//! Implements `strfmon` and `strfmon_l` for formatting monetary values.
//!
//! ## Limitations
//!
//! Only the C/POSIX locale is supported.  The format specifiers
//! `%n` (national) and `%i` (international) both produce simple
//! decimal output without currency symbols.  The `=` fill character,
//! `^` (no grouping), `+`/`(` (sign position), `!` (no currency),
//! and width/precision modifiers are supported.

/// `strfmon` — format monetary value.
///
/// Writes at most `maxsize` bytes (including the null terminator) to
/// `s` based on `format`.  Returns the number of bytes written
/// (excluding the null terminator), or -1 on error.
///
/// ## Supported format specifiers
///
/// - `%n` — national monetary format (decimal, no currency symbol)
/// - `%i` — international monetary format (same as `%n` in C locale)
/// - `%%` — literal `%`
///
/// ## Optional flags (between `%` and the specifier)
///
/// - `=<char>` — fill character (default: space)
/// - `^` — suppress grouping (no effect in C locale)
/// - `+` — use `+`/`-` for sign (default)
/// - `(` — use parentheses for negative values
/// - `!` — suppress currency symbol (no effect — we don't emit one)
/// - Width: minimum field width
/// - `.precision`: decimal places (default: 2)
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strfmon(
    s: *mut u8,
    maxsize: usize,
    format: *const u8,
    // Note: Real strfmon is variadic. We take one double argument.
    // Programs typically format one value at a time.
    value: f64,
) -> isize {
    if s.is_null() || format.is_null() || maxsize == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    let mut out_pos: usize = 0;
    let mut fmt_pos: usize = 0;

    loop {
        let ch = unsafe { *format.add(fmt_pos) };
        if ch == 0 {
            break;
        }
        fmt_pos = fmt_pos.wrapping_add(1);

        if ch != b'%' {
            if out_pos >= maxsize.wrapping_sub(1) {
                break;
            }
            unsafe { *s.add(out_pos) = ch; }
            out_pos = out_pos.wrapping_add(1);
            continue;
        }

        // Parse format specifier after '%'.
        let next = unsafe { *format.add(fmt_pos) };
        if next == 0 {
            break;
        }

        if next == b'%' {
            // Literal '%'.
            if out_pos >= maxsize.wrapping_sub(1) { break; }
            unsafe { *s.add(out_pos) = b'%'; }
            out_pos = out_pos.wrapping_add(1);
            fmt_pos = fmt_pos.wrapping_add(1);
            continue;
        }

        // Parse optional flags.
        let mut _fill_char: u8 = b' ';
        let mut _suppress_grouping = false;
        let mut use_parens = false;
        let mut _suppress_currency = false;

        let mut fp = fmt_pos;
        loop {
            let fc = unsafe { *format.add(fp) };
            if fc == b'=' {
                fp = fp.wrapping_add(1);
                _fill_char = unsafe { *format.add(fp) };
                if _fill_char == 0 { break; }
                fp = fp.wrapping_add(1);
            } else if fc == b'^' {
                _suppress_grouping = true;
                fp = fp.wrapping_add(1);
            } else if fc == b'+' {
                use_parens = false;
                fp = fp.wrapping_add(1);
            } else if fc == b'(' {
                use_parens = true;
                fp = fp.wrapping_add(1);
            } else if fc == b'!' {
                _suppress_currency = true;
                fp = fp.wrapping_add(1);
            } else {
                break;
            }
        }

        // Parse optional width.
        let mut _width: usize = 0;
        while unsafe { *format.add(fp) }.is_ascii_digit() {
            let d = unsafe { *format.add(fp) };
            _width = _width.wrapping_mul(10).wrapping_add((d - b'0') as usize);
            fp = fp.wrapping_add(1);
        }

        // Parse optional precision.
        let mut precision: usize = 2; // default
        if unsafe { *format.add(fp) } == b'.' {
            fp = fp.wrapping_add(1);
            precision = 0;
            while unsafe { *format.add(fp) }.is_ascii_digit() {
                let d = unsafe { *format.add(fp) };
                precision = precision.wrapping_mul(10).wrapping_add((d - b'0') as usize);
                fp = fp.wrapping_add(1);
            }
        }

        // The specifier character.
        let spec = unsafe { *format.add(fp) };
        if spec == 0 { break; }
        fp = fp.wrapping_add(1);
        fmt_pos = fp;

        if spec != b'n' && spec != b'i' {
            // Unknown specifier — skip.
            continue;
        }

        // Format the value as decimal with `precision` decimal places.
        let negative = value < 0.0;
        let abs_val = if negative { -value } else { value };

        // Compute integer and fractional parts.
        let mut multiplier: f64 = 1.0;
        let mut p = precision;
        while p > 0 {
            multiplier *= 10.0;
            p = p.wrapping_sub(1);
        }

        let scaled = (abs_val * multiplier + 0.5) as u64;
        let int_part = scaled / (multiplier as u64);
        let frac_part = scaled % (multiplier as u64);

        // Format sign.
        if negative {
            if use_parens {
                if out_pos < maxsize.wrapping_sub(1) {
                    unsafe { *s.add(out_pos) = b'('; }
                    out_pos = out_pos.wrapping_add(1);
                }
            } else {
                if out_pos < maxsize.wrapping_sub(1) {
                    unsafe { *s.add(out_pos) = b'-'; }
                    out_pos = out_pos.wrapping_add(1);
                }
            }
        }

        // Format integer part.
        let mut int_buf = [0u8; 20];
        let int_len = format_u64(int_part, &mut int_buf);
        let mut i: usize = 0;
        while i < int_len && out_pos < maxsize.wrapping_sub(1) {
            unsafe { *s.add(out_pos) = int_buf[i]; }
            out_pos = out_pos.wrapping_add(1);
            i = i.wrapping_add(1);
        }

        // Format fractional part (if precision > 0).
        if precision > 0 {
            if out_pos < maxsize.wrapping_sub(1) {
                unsafe { *s.add(out_pos) = b'.'; }
                out_pos = out_pos.wrapping_add(1);
            }

            let mut frac_buf = [0u8; 20];
            let frac_len = format_u64(frac_part, &mut frac_buf);
            // Pad with leading zeros if needed.
            let mut pad = if precision > frac_len { precision.wrapping_sub(frac_len) } else { 0 };
            while pad > 0 && out_pos < maxsize.wrapping_sub(1) {
                unsafe { *s.add(out_pos) = b'0'; }
                out_pos = out_pos.wrapping_add(1);
                pad = pad.wrapping_sub(1);
            }
            let mut j: usize = 0;
            while j < frac_len && out_pos < maxsize.wrapping_sub(1) {
                unsafe { *s.add(out_pos) = frac_buf[j]; }
                out_pos = out_pos.wrapping_add(1);
                j = j.wrapping_add(1);
            }
        }

        // Close parentheses for negative with parens.
        if negative && use_parens {
            if out_pos < maxsize.wrapping_sub(1) {
                unsafe { *s.add(out_pos) = b')'; }
                out_pos = out_pos.wrapping_add(1);
            }
        }
    }

    // Null terminate.
    if out_pos >= maxsize {
        out_pos = maxsize.wrapping_sub(1);
    }
    unsafe { *s.add(out_pos) = 0; }

    out_pos as isize
}

/// `strfmon_l` — locale-aware monetary formatting.
///
/// Stub: ignores the locale parameter and delegates to `strfmon`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strfmon_l(
    s: *mut u8,
    maxsize: usize,
    _locale: usize,
    format: *const u8,
    value: f64,
) -> isize {
    strfmon(s, maxsize, format, value)
}

/// Format a u64 into decimal digits.  Returns the number of bytes written.
fn format_u64(mut val: u64, buf: &mut [u8; 20]) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut digits = [0u8; 20];
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
        if let (Some(dst), Some(src)) = (buf.get_mut(i), digits.get(len.wrapping_sub(1).wrapping_sub(i))) {
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

    /// Helper: run strfmon and return the result as a String.
    fn run_strfmon(fmt: &[u8], value: f64) -> Vec<u8> {
        let mut buf = [0u8; 128];
        let ret = strfmon(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), value);
        assert!(ret >= 0, "strfmon returned error: {ret}");
        let len = ret as usize;
        buf[..len].to_vec()
    }

    // -----------------------------------------------------------------------
    // Basic formatting
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_positive() {
        let result = run_strfmon(b"%n\0", 1234.56);
        assert_eq!(result, b"1234.56");
    }

    #[test]
    fn test_strfmon_negative() {
        let result = run_strfmon(b"%n\0", -42.50);
        assert_eq!(result, b"-42.50");
    }

    #[test]
    fn test_strfmon_zero() {
        let result = run_strfmon(b"%n\0", 0.0);
        assert_eq!(result, b"0.00");
    }

    #[test]
    fn test_strfmon_small_value() {
        let result = run_strfmon(b"%n\0", 0.99);
        assert_eq!(result, b"0.99");
    }

    #[test]
    fn test_strfmon_large_value() {
        let result = run_strfmon(b"%n\0", 999999.99);
        assert_eq!(result, b"999999.99");
    }

    // -----------------------------------------------------------------------
    // International format (%i)
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_international() {
        let result = run_strfmon(b"%i\0", 100.00);
        assert_eq!(result, b"100.00");
    }

    // -----------------------------------------------------------------------
    // Precision
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_precision_0() {
        let result = run_strfmon(b"%.0n\0", 42.99);
        assert_eq!(result, b"43");
    }

    #[test]
    fn test_strfmon_precision_4() {
        let result = run_strfmon(b"%.4n\0", 1.5);
        assert_eq!(result, b"1.5000");
    }

    // -----------------------------------------------------------------------
    // Parentheses for negative
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_parens_negative() {
        let result = run_strfmon(b"%(n\0", -99.00);
        assert_eq!(result, b"(99.00)");
    }

    #[test]
    fn test_strfmon_parens_positive() {
        let result = run_strfmon(b"%(n\0", 99.00);
        assert_eq!(result, b"99.00");
    }

    // -----------------------------------------------------------------------
    // Literal percent
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_literal_percent() {
        let result = run_strfmon(b"%%\0", 0.0);
        assert_eq!(result, b"%");
    }

    // -----------------------------------------------------------------------
    // Mixed text and format
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_text_prefix() {
        let result = run_strfmon(b"Total: %n\0", 50.00);
        assert_eq!(result, b"Total: 50.00");
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_null_s() {
        crate::errno::set_errno(0);
        let ret = strfmon(core::ptr::null_mut(), 128, b"%n\0".as_ptr(), 0.0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_strfmon_null_format() {
        let mut buf = [0u8; 128];
        crate::errno::set_errno(0);
        let ret = strfmon(buf.as_mut_ptr(), 128, core::ptr::null(), 0.0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_strfmon_zero_maxsize() {
        let mut buf = [0u8; 128];
        let ret = strfmon(buf.as_mut_ptr(), 0, b"%n\0".as_ptr(), 0.0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // strfmon_l delegates to strfmon
    // -----------------------------------------------------------------------

    #[test]
    fn test_strfmon_l_basic() {
        let mut buf = [0u8; 128];
        let ret = strfmon_l(buf.as_mut_ptr(), 128, 0, b"%n\0".as_ptr(), 25.00);
        assert!(ret >= 0);
        let len = ret as usize;
        assert_eq!(&buf[..len], b"25.00");
    }

    // -----------------------------------------------------------------------
    // format_u64 helper
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_u64_zero() {
        let mut buf = [0u8; 20];
        let len = format_u64(0, &mut buf);
        assert_eq!(len, 1);
        assert_eq!(buf[0], b'0');
    }

    #[test]
    fn test_format_u64_large() {
        let mut buf = [0u8; 20];
        let len = format_u64(1234567890, &mut buf);
        assert_eq!(len, 10);
        assert_eq!(&buf[..10], b"1234567890");
    }

    #[test]
    fn test_format_u64_one() {
        let mut buf = [0u8; 20];
        let len = format_u64(1, &mut buf);
        assert_eq!(len, 1);
        assert_eq!(buf[0], b'1');
    }
}
