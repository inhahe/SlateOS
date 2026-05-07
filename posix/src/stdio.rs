//! Basic C standard I/O functions.
//!
//! Implements a minimal subset of `<stdio.h>`: `putchar`, `puts`,
//! `fputs`, `fputc`, `fwrite`, `fread`, `perror`.
//!
//! Full `printf`/`snprintf` with C variadic args requires either the
//! nightly `c_variadic` feature or assembly trampolines.  Those will
//! be provided when we port musl.  This module covers the non-variadic
//! output functions that are most commonly needed.

/// File number for stdin.
const STDIN: i32 = 0;
/// File number for stdout.
const STDOUT: i32 = 1;
/// File number for stderr.
const STDERR: i32 = 2;

/// EOF indicator.
pub const EOF: i32 = -1;

/// Standard C FILE stream identifiers.
///
/// In a real libc these are opaque structs with buffering state.
/// We use small integers cast to pointers as stream identifiers,
/// dispatching to fd numbers.  This is enough for programs that
/// just pass stdin/stdout/stderr to fputs/fwrite/etc.
const STDOUT_FILENO: usize = 1;
const STDERR_FILENO: usize = 2;

/// Write a character to stdout.
///
/// Returns the character written as an unsigned char cast to int,
/// or EOF (-1) on error.
#[unsafe(no_mangle)]
pub extern "C" fn putchar(c: i32) -> i32 {
    let byte = c as u8;
    let ret = crate::file::write(STDOUT, &raw const byte, 1);
    if ret < 0 { EOF } else { i32::from(byte) }
}

/// Write a string to stdout followed by a newline.
///
/// Returns a non-negative value on success, EOF (-1) on error.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puts(s: *const u8) -> i32 {
    if s.is_null() {
        return EOF;
    }
    let len = unsafe { crate::string::strlen(s) };
    let ret = crate::file::write(STDOUT, s, len);
    if ret < 0 {
        return EOF;
    }
    // Write trailing newline.
    let nl = b'\n';
    let ret2 = crate::file::write(STDOUT, &raw const nl, 1);
    if ret2 < 0 { EOF } else { 0 }
}

/// Write a string to a stream.
///
/// Returns a non-negative value on success, EOF (-1) on error.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
/// `stream` must be a valid FILE pointer (stdin, stdout, or stderr).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fputs(s: *const u8, stream: *mut u8) -> i32 {
    if s.is_null() {
        return EOF;
    }
    let fd = stream_to_fd(stream);
    let len = unsafe { crate::string::strlen(s) };
    let ret = crate::file::write(fd, s, len);
    if ret < 0 { EOF } else { 0 }
}

/// Write a character to a stream.
#[unsafe(no_mangle)]
pub extern "C" fn fputc(c: i32, stream: *mut u8) -> i32 {
    let byte = c as u8;
    let fd = stream_to_fd(stream);
    let ret = crate::file::write(fd, &raw const byte, 1);
    if ret < 0 { EOF } else { i32::from(byte) }
}

/// Read a character from a stream.
///
/// Returns the character read as an unsigned char cast to int,
/// or EOF (-1) on error or end of file.
#[unsafe(no_mangle)]
pub extern "C" fn fgetc(stream: *mut u8) -> i32 {
    let fd = stream_to_fd(stream);
    let mut byte: u8 = 0;
    let ret = crate::file::read(fd, &raw mut byte, 1);
    if ret <= 0 { EOF } else { i32::from(byte) }
}

/// Read a character from stdin.
#[unsafe(no_mangle)]
pub extern "C" fn getchar() -> i32 {
    let mut byte: u8 = 0;
    let ret = crate::file::read(STDIN, &raw mut byte, 1);
    if ret <= 0 { EOF } else { i32::from(byte) }
}

/// Write data to a stream.
///
/// Returns the number of complete elements written.
///
/// # Safety
///
/// `ptr` must be valid for `size * nmemb` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fwrite(
    ptr: *const u8,
    size: usize,
    nmemb: usize,
    stream: *mut u8,
) -> usize {
    if ptr.is_null() || size == 0 || nmemb == 0 {
        return 0;
    }
    let fd = stream_to_fd(stream);
    let total = size.saturating_mul(nmemb);
    let ret = crate::file::write(fd, ptr, total);
    if ret < 0 {
        0
    } else {
        // size > 0 guaranteed by early return above.
        #[allow(clippy::arithmetic_side_effects)]
        { (ret as usize) / size }
    }
}

/// Read data from a stream.
///
/// Returns the number of complete elements read.
///
/// # Safety
///
/// `ptr` must be valid for `size * nmemb` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fread(
    ptr: *mut u8,
    size: usize,
    nmemb: usize,
    stream: *mut u8,
) -> usize {
    if ptr.is_null() || size == 0 || nmemb == 0 {
        return 0;
    }
    let fd = stream_to_fd(stream);
    let total = size.saturating_mul(nmemb);
    let ret = crate::file::read(fd, ptr, total);
    if ret < 0 {
        0
    } else {
        // size > 0 guaranteed by early return above.
        #[allow(clippy::arithmetic_side_effects)]
        { (ret as usize) / size }
    }
}

/// Print an error message to stderr.
///
/// If `s` is non-null and non-empty, prints "s: error_string\n".
/// Otherwise just prints "error_string\n".
///
/// # Safety
///
/// `s` (if non-null) must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn perror(s: *const u8) {
    let err = crate::errno::get_errno();
    let msg = crate::string::strerror(err);

    if !s.is_null() && unsafe { *s } != 0 {
        let slen = unsafe { crate::string::strlen(s) };
        let _ = crate::file::write(STDERR, s, slen);
        let _ = crate::file::write(STDERR, c": ".as_ptr().cast::<u8>(), 2);
    }

    if !msg.is_null() {
        let mlen = unsafe { crate::string::strlen(msg) };
        let _ = crate::file::write(STDERR, msg, mlen);
    }

    let nl = b'\n';
    let _ = crate::file::write(STDERR, &raw const nl, 1);
}

// ---------------------------------------------------------------------------
// FILE* symbols
// ---------------------------------------------------------------------------

/// Convert a FILE* stream pointer to a file descriptor number.
///
/// We use the convention that stdout = 1, stderr = 2 as pointer values.
#[inline]
fn stream_to_fd(stream: *mut u8) -> i32 {
    match stream as usize {
        STDOUT_FILENO => STDOUT,
        STDERR_FILENO => STDERR,
        0 => STDIN,
        other => other as i32, // Treat as raw fd.
    }
}

/// Provide global FILE* symbols for stdout, stderr, stdin.
///
/// C programs access these as `extern FILE *stdout;`.
/// We set them to our sentinel values.
#[unsafe(no_mangle)]
pub static stdout: usize = STDOUT_FILENO;

#[unsafe(no_mangle)]
pub static stderr: usize = STDERR_FILENO;

#[unsafe(no_mangle)]
pub static stdin: usize = 0;
