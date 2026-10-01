// Every index is into the ten-byte digit buffer, counted down from its
// length while a digit remains, and every sum is of the message's part
// lengths, each a string's own. Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! C assertion support: `__assert_fail`, what `<assert.h>`'s `assert` calls
//! when its expression is false, and glibc's two others -- `__assert_perror_fail`,
//! which GNU's `assert_perror` calls, and `__assert`, BSD's older entry --
//! each writing glibc's message to `stderr` and ending the process with
//! `abort`.
//!
//! The messages are glibc 2.39's, as a WSL probe printed them (2026-09-30):
//!
//! ```text
//! prog: file.c:12: main: Assertion `x == 1' failed.
//! prog: file.c:7: fn: Unexpected error: No such file or directory.
//! ```
//!
//! -- the program's short name (`__progname`) and `": "` first unless that
//! name is empty; the function and `": "` only when there is one; `(null)`
//! for a NULL text, as glibc's `printf` prints one. The message is built
//! whole and written to `stderr` in one piece, and `stderr` flushed, so it
//! reaches the reader as one write even with `stderr` unbuffered; when the
//! memory to build it in cannot be had, `Unexpected error.` is written
//! instead, as glibc does.
//!
//! Until 2026-09-30 this wrote `Assertion failed: EXPR, file FILE, line
//! LINE, function FUNC` -- no C library's form -- straight to descriptor 2,
//! and `__assert_perror_fail` and `__assert` were missing.
//!
//! One deviation: glibc's `__assert`, which its source makes `__assert_fail`
//! with no function, dies of `SIGSEGV` in the oracle's build without writing
//! anything; this one writes that message and aborts.

use crate::stdio;

/// A C string's bytes, or `(null)` for NULL, as glibc's `%s` prints it.
///
/// # Safety
///
/// `s` is NULL or a NUL-terminated string.
unsafe fn text<'a>(s: *const u8) -> &'a [u8] {
    if s.is_null() {
        return b"(null)";
    }
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) }
}

/// What follows the place in the message.
enum Tail<'a> {
    /// `Assertion `EXPR' failed.`
    Assertion(&'a [u8]),
    /// `Unexpected error: TEXT.` -- `strerror`'s text for the error.
    Error(&'a [u8]),
}

/// `line` in decimal.
fn decimal(mut line: u32, buf: &mut [u8; 10]) -> &[u8] {
    let mut k = buf.len();
    loop {
        k -= 1;
        // `line % 10` is a digit.
        #[allow(clippy::cast_possible_truncation)]
        let digit = (line % 10) as u8;
        buf[k] = b'0' + digit;
        line /= 10;
        if line == 0 {
            break;
        }
    }
    &buf[k..]
}

/// glibc's message into `out` (a `FILE *`): `prog: file:line: func: ` --
/// `prog: ` only for a name that is not empty, `func: ` only for a function
/// -- then the tail and a newline, in one write, and the stream flushed.
/// `Unexpected error.` alone, straight to descriptor 2, when the memory for
/// it cannot be had.
fn emit(
    out: *mut u8,
    progname: &[u8],
    file: &[u8],
    line: u32,
    function: Option<&[u8]>,
    tail: &Tail<'_>,
) {
    let mut digits = [0u8; 10];
    let line = decimal(line, &mut digits);
    let (lead, what, trail): (&[u8], &[u8], &[u8]) = match tail {
        Tail::Assertion(e) => (b"Assertion `", e, b"' failed.\n"),
        Tail::Error(e) => (b"Unexpected error: ", e, b".\n"),
    };
    let sep: &[u8] = if progname.is_empty() { b"" } else { b": " };
    let (func, func_sep): (&[u8], &[u8]) = match function {
        Some(f) => (f, b": "),
        None => (b"", b""),
    };
    let parts: [&[u8]; 11] = [
        progname, sep, file, b":", line, b": ", func, func_sep, lead, what, trail,
    ];
    let total = parts.iter().fold(0usize, |n, p| n.saturating_add(p.len()));
    let buf = crate::malloc::malloc(total);
    if buf.is_null() {
        let msg = b"Unexpected error.\n";
        // Nothing is left to do about a failed write: the process aborts next.
        let _ = crate::file::write(2, msg.as_ptr(), msg.len());
        return;
    }
    let mut at = 0usize;
    for p in parts {
        // SAFETY: `buf` holds `total` bytes, the parts' lengths summed.
        unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), buf.add(at), p.len()) };
        at += p.len();
    }
    // SAFETY: `buf` holds `total` bytes; `out` is a stream. What a failed
    // write loses is the message itself: the process aborts next.
    unsafe {
        let _ = stdio::fwrite(buf, 1, total, out);
        crate::malloc::free(buf);
    }
    // As the write: a failure has no remedy, the process aborting next.
    let _ = stdio::fflush(out);
}

/// The program's short name, as `__libc_start_main` set it.
fn progname<'a>() -> &'a [u8] {
    // SAFETY: a plain read of the pointer `__libc_start_main` set; it points
    // at a NUL-terminated name (or is NULL, `(null)`).
    unsafe { text(core::ptr::addr_of!(crate::crt::__progname).read()) }
}

/// Called when a C `assert` fails: glibc's message for `assertion` at
/// `file:line` in `function` (NULL for none) on `stderr`, then `abort`.
///
/// # Safety
///
/// Each pointer is NULL or a NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __assert_fail(
    assertion: *const u8,
    file: *const u8,
    line: u32,
    function: *const u8,
) -> ! {
    // SAFETY: the caller's contract.
    let (assertion, file) = unsafe { (text(assertion), text(file)) };
    // SAFETY: as above.
    let function = (!function.is_null()).then(|| unsafe { text(function) });
    emit(
        stdio::stderr_stream(),
        progname(),
        file,
        line,
        function,
        &Tail::Assertion(assertion),
    );
    crate::unistd::abort();
}

/// Called when GNU's `assert_perror(errnum)` finds `errnum` non-zero:
/// glibc's `Unexpected error: ` message with `strerror`'s text for it, then
/// `abort`.
///
/// # Safety
///
/// `file` and `function` are NULL or NUL-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __assert_perror_fail(
    errnum: i32,
    file: *const u8,
    line: u32,
    function: *const u8,
) -> ! {
    // SAFETY: `strerror` answers a NUL-terminated string; `file` is the
    // caller's.
    let (error, file) = unsafe { (text(crate::string::strerror(errnum)), text(file)) };
    // SAFETY: the caller's contract.
    let function = (!function.is_null()).then(|| unsafe { text(function) });
    emit(
        stdio::stderr_stream(),
        progname(),
        file,
        line,
        function,
        &Tail::Error(error),
    );
    crate::unistd::abort();
}

/// BSD's older assertion entry: `__assert_fail` with no function. `line`
/// is an `int`, and prints as glibc's `%u` prints it (-5 as 4294967291).
///
/// # Safety
///
/// `assertion` and `file` are NULL or NUL-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __assert(assertion: *const u8, file: *const u8, line: i32) -> ! {
    // SAFETY: the caller's contract.
    unsafe { __assert_fail(assertion, file, line.cast_unsigned(), core::ptr::null()) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::capture::Pair;
    use std::string::String;

    /// What `emit` writes for these parts, as the reader of `stderr` sees it.
    fn written(
        progname: &[u8],
        file: &[u8],
        line: u32,
        function: Option<&[u8]>,
        tail: &Tail<'_>,
    ) -> String {
        let pair = Pair::new();
        emit(pair.stderr, progname, file, line, function, tail);
        pair.text()
    }

    /// A probe: the program's name, file, line, function and tail, and the
    /// message glibc wrote for them.
    type Case<'a> = (&'a [u8], &'a [u8], u32, Option<&'a [u8]>, Tail<'a>, &'a str);

    /// The WSL probe's answers (glibc 2.39, 2026-09-30), `assert_probe` its
    /// program's name.
    #[test]
    fn messages_are_glibcs() {
        let p: &[u8] = b"assert_probe";
        let cases: [Case<'_>; 9] = [
            (
                p,
                b"file.c",
                12,
                Some(b"main"),
                Tail::Assertion(b"x == 1"),
                "assert_probe: file.c:12: main: Assertion `x == 1' failed.\n",
            ),
            (
                p,
                b"file.c",
                12,
                None,
                Tail::Assertion(b"x == 1"),
                "assert_probe: file.c:12: Assertion `x == 1' failed.\n",
            ),
            (
                p,
                b"(null)",
                0,
                None,
                Tail::Assertion(b"(null)"),
                "assert_probe: (null):0: Assertion `(null)' failed.\n",
            ),
            (
                p,
                b"",
                u32::MAX,
                Some(b""),
                Tail::Assertion(b""),
                "assert_probe: :4294967295: : Assertion `' failed.\n",
            ),
            (
                p,
                b"file.c",
                7,
                Some(b"fn"),
                Tail::Error(b"No such file or directory"),
                "assert_probe: file.c:7: fn: Unexpected error: No such file or directory.\n",
            ),
            (
                p,
                b"file.c",
                7,
                None,
                Tail::Error(b"Success"),
                "assert_probe: file.c:7: Unexpected error: Success.\n",
            ),
            (
                p,
                b"f.c",
                1,
                Some(b"g"),
                Tail::Error(b"Unknown error 99999"),
                "assert_probe: f.c:1: g: Unexpected error: Unknown error 99999.\n",
            ),
            (
                b"",
                b"f.c",
                2,
                Some(b"h"),
                Tail::Assertion(b"e"),
                "f.c:2: h: Assertion `e' failed.\n",
            ),
            (
                b"renamed",
                b"f.c",
                2,
                Some(b"h"),
                Tail::Assertion(b"e"),
                "renamed: f.c:2: h: Assertion `e' failed.\n",
            ),
        ];
        for (name, file, line, function, tail, want) in &cases {
            assert_eq!(written(name, file, *line, *function, tail), *want);
        }
    }

    /// The texts `__assert_perror_fail` takes from `strerror` are the ones
    /// the probe printed.
    #[test]
    fn error_texts_are_strerrors() {
        for (e, want) in [
            (crate::errno::ENOENT, &b"No such file or directory"[..]),
            (0, b"Success"),
            (99999, b"Unknown error 99999"),
        ] {
            // SAFETY: `strerror` answers a NUL-terminated string.
            assert_eq!(
                unsafe { text(crate::string::strerror(e)) },
                want,
                "strerror({e})"
            );
        }
    }

    #[test]
    fn a_null_text_reads_as_glibcs_null() {
        // SAFETY: NULL is allowed.
        assert_eq!(unsafe { text(core::ptr::null()) }, b"(null)");
        // SAFETY: a C string.
        assert_eq!(unsafe { text(c"x".as_ptr().cast()) }, b"x");
    }

    #[test]
    fn lines_print_in_decimal() {
        let mut b = [0u8; 10];
        for (v, want) in [
            (0u32, &b"0"[..]),
            (7, b"7"),
            (42, b"42"),
            (65535, b"65535"),
            (u32::MAX, b"4294967295"),
        ] {
            assert_eq!(decimal(v, &mut b), want);
        }
        // `__assert`'s -5, as `%u`.
        assert_eq!(decimal((-5i32).cast_unsigned(), &mut b), b"4294967291");
    }
}
