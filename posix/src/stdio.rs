//! C standard I/O functions.
//!
//! Implements `<stdio.h>`: `putchar`, `puts`, `fputs`, `fputc`, `fgetc`,
//! `getchar`, `getc`, `putc`, `ungetc`, `fwrite`, `fread`, `fgets`,
//! `fopen`, `fdopen`, `freopen`, `fclose`, `fflush`, `fseek`, `ftell`,
//! `rewind`, `fileno`, `feof`, `ferror`, `clearerr`, `perror`,
//! `remove`, `tmpnam`, `setvbuf`, `setbuf`, `popen`, `pclose`,
//! `getline`, `getdelim`.
//!
//! printf/fprintf/sprintf/snprintf are in the `printf` module (via
//! assembly trampoline).

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
    // Check for a pushed-back byte from ungetc.
    if let Some(ch) = consume_ungetc(fd) {
        return i32::from(ch);
    }
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

// ---------------------------------------------------------------------------
// fopen / fclose / fflush
// ---------------------------------------------------------------------------

/// Open a file as a stdio stream.
///
/// Returns a FILE* (which is really just the fd cast to a pointer).
/// Returns null on error with errno set.
///
/// Supported modes: `"r"`, `"w"`, `"a"`, `"r+"`, `"w+"`, `"a+"`.
/// The `"b"` suffix is accepted and ignored (binary mode is the default).
///
/// # Safety
///
/// `path` and `mode` must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fopen(path: *const u8, mode: *const u8) -> *mut u8 {
    if path.is_null() || mode.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    let flags = mode_to_flags(mode);
    if flags < 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    let fd = crate::file::open(path, flags, 0o666);
    if fd < 0 {
        return core::ptr::null_mut();
    }

    fd as usize as *mut u8
}

/// Close a stdio stream.
///
/// Returns 0 on success, EOF on error.
#[unsafe(no_mangle)]
pub extern "C" fn fclose(stream: *mut u8) -> i32 {
    let fd = stream_to_fd(stream);
    // Don't close stdin/stdout/stderr.
    if fd <= STDERR {
        return 0;
    }
    let ret = crate::file::close(fd);
    if ret < 0 { EOF } else { 0 }
}

/// Flush a stream (no-op).
///
/// We have no buffering, so fflush is always a no-op that succeeds.
/// If stream is null, "flushes all open streams" — also a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn fflush(_stream: *mut u8) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// fgets / getline
// ---------------------------------------------------------------------------

/// Read a line from a stream.
///
/// Reads at most `size - 1` characters into `buf`, stopping at a
/// newline or EOF.  The newline is included if read.  The string is
/// always null-terminated.
///
/// Returns `buf` on success, null on error or EOF with no data read.
#[unsafe(no_mangle)]
pub extern "C" fn fgets(buf: *mut u8, size: i32, stream: *mut u8) -> *mut u8 {
    if buf.is_null() || size <= 0 {
        return core::ptr::null_mut();
    }

    let fd = stream_to_fd(stream);
    let max = (size as usize).wrapping_sub(1); // Leave room for null.
    let mut pos: usize = 0;

    while pos < max {
        let mut byte: u8 = 0;
        let n = crate::file::read(fd, &raw mut byte, 1);
        if n <= 0 {
            break; // EOF or error.
        }
        // SAFETY: pos < max < size, so buf.add(pos) is valid.
        unsafe { *buf.add(pos) = byte; }
        pos = pos.wrapping_add(1);
        if byte == b'\n' {
            break;
        }
    }

    if pos == 0 {
        return core::ptr::null_mut(); // Nothing read.
    }

    // Null-terminate.
    // SAFETY: pos <= max = size-1, so buf.add(pos) is within bounds.
    unsafe { *buf.add(pos) = 0; }

    buf
}

// ---------------------------------------------------------------------------
// fseek / ftell / rewind
// ---------------------------------------------------------------------------

/// Seek constants (match POSIX).
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

/// Seek within a stream.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn fseek(stream: *mut u8, offset: i64, whence: i32) -> i32 {
    let fd = stream_to_fd(stream);
    let ret = crate::file::lseek(fd, offset, whence);
    if ret < 0 { -1 } else { 0 }
}

/// Get the current position in a stream.
///
/// Returns the current offset, or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn ftell(stream: *mut u8) -> i64 {
    let fd = stream_to_fd(stream);
    crate::file::lseek(fd, 0, SEEK_CUR)
}

/// Rewind a stream to the beginning.
///
/// Equivalent to `fseek(stream, 0, SEEK_SET)` but returns void.
#[unsafe(no_mangle)]
pub extern "C" fn rewind(stream: *mut u8) {
    let fd = stream_to_fd(stream);
    let _ = crate::file::lseek(fd, 0, SEEK_SET);
}

// ---------------------------------------------------------------------------
// fileno / feof / ferror / clearerr
// ---------------------------------------------------------------------------

/// Get the file descriptor for a stream.
#[unsafe(no_mangle)]
pub extern "C" fn fileno(stream: *mut u8) -> i32 {
    stream_to_fd(stream)
}

/// Check end-of-file indicator for a stream.
///
/// Stub: always returns 0 (not at EOF).  We don't track per-stream
/// EOF state since we have no real FILE struct.
#[unsafe(no_mangle)]
pub extern "C" fn feof(_stream: *mut u8) -> i32 {
    0
}

/// Check error indicator for a stream.
///
/// Stub: always returns 0 (no error).
#[unsafe(no_mangle)]
pub extern "C" fn ferror(_stream: *mut u8) -> i32 {
    0
}

/// Clear error and EOF indicators for a stream.
///
/// Stub: no-op.
#[unsafe(no_mangle)]
pub extern "C" fn clearerr(_stream: *mut u8) {
}

/// Remove a file.
///
/// Wrapper around `unlink`.
#[unsafe(no_mangle)]
pub extern "C" fn remove(path: *const u8) -> i32 {
    crate::file::unlink(path)
}

/// Rename a file.
///
/// Wrapper around the file::rename function.
#[unsafe(no_mangle)]
pub extern "C" fn stdio_rename(old: *const u8, new: *const u8) -> i32 {
    crate::file::rename(old, new)
}

/// Create a temporary filename (not thread-safe).
///
/// Stub: returns null (not implemented).
#[unsafe(no_mangle)]
pub extern "C" fn tmpnam(_s: *mut u8) -> *mut u8 {
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Additional FILE operations
// ---------------------------------------------------------------------------

/// Associate a stream with an existing file descriptor.
///
/// Returns a FILE* (which is just the fd cast to a pointer in our
/// implementation), or NULL on error.
#[unsafe(no_mangle)]
pub extern "C" fn fdopen(fd: i32, _mode: *const u8) -> *mut u8 {
    // In our implementation, FILE* IS the fd.
    // We just verify the fd is valid (non-negative).
    if fd < 0 {
        crate::errno::set_errno(crate::errno::EBADF);
        return core::ptr::null_mut();
    }
    fd as usize as *mut u8
}

/// Reopen a stream with a different file or mode.
///
/// If `path` is non-null, closes the old stream and opens the new path.
/// If `path` is null, attempts to change the mode (not fully supported).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn freopen(
    path: *const u8,
    mode: *const u8,
    stream: *mut u8,
) -> *mut u8 {
    if mode.is_null() {
        return core::ptr::null_mut();
    }

    let old_fd = stream as usize as i32;

    if path.is_null() {
        // Mode change only — not supported, return the same stream.
        return stream;
    }

    // Close the old fd (unless it's stdin/stdout/stderr, which we keep).
    if old_fd > STDERR {
        crate::file::close(old_fd);
    }

    // Open the new file.
    let flags = mode_to_flags(mode);
    if flags < 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    let new_fd = crate::file::open(path, flags, 0o666);
    if new_fd < 0 {
        return core::ptr::null_mut();
    }

    // If the caller wants the new file on the old fd number (e.g.,
    // freopen on stdout), dup2 it.
    if new_fd != old_fd && old_fd <= STDERR {
        let duped = crate::file::dup2(new_fd, old_fd);
        crate::file::close(new_fd);
        if duped < 0 {
            return core::ptr::null_mut();
        }
        return old_fd as usize as *mut u8;
    }

    new_fd as usize as *mut u8
}

/// Push a character back onto the input stream.
///
/// In our unbuffered implementation, we use a static one-byte buffer
/// per fd (up to 16 fds can have an ungetc'd character).
#[unsafe(no_mangle)]
pub extern "C" fn ungetc(ch: i32, stream: *mut u8) -> i32 {
    if ch == EOF {
        return EOF;
    }
    let fd = stream as usize as i32;
    if fd < 0 {
        return EOF;
    }

    // Store the pushed-back byte for this fd.
    let idx = fd as usize;
    if idx >= UNGETC_SLOTS {
        return EOF;
    }

    // SAFETY: Single-threaded access.
    unsafe {
        if let Some(slot) = (*core::ptr::addr_of_mut!(UNGETC_BUF)).get_mut(idx) {
            *slot = (ch & 0xFF) as i16;
        }
    }
    ch & 0xFF
}

/// Number of ungetc slots (max fd for ungetc support).
const UNGETC_SLOTS: usize = 64;

/// Per-fd ungetc buffer.  -1 means no pushed-back byte.
static mut UNGETC_BUF: [i16; UNGETC_SLOTS] = [-1; UNGETC_SLOTS];

/// Check for and consume an ungetc'd byte.
fn consume_ungetc(fd: i32) -> Option<u8> {
    let idx = fd as usize;
    if idx >= UNGETC_SLOTS {
        return None;
    }
    // SAFETY: Single-threaded access.
    let val = unsafe { (*core::ptr::addr_of!(UNGETC_BUF)).get(idx).copied().unwrap_or(-1) };
    if val < 0 {
        return None;
    }
    unsafe {
        if let Some(slot) = (*core::ptr::addr_of_mut!(UNGETC_BUF)).get_mut(idx) {
            *slot = -1; // Consume it.
        }
    }
    Some(val as u8)
}

/// Read a character from a stream (function version of getc macro).
#[unsafe(no_mangle)]
pub extern "C" fn getc(stream: *mut u8) -> i32 {
    fgetc(stream)
}

/// Write a character to a stream (function version of putc macro).
#[unsafe(no_mangle)]
pub extern "C" fn putc(ch: i32, stream: *mut u8) -> i32 {
    fputc(ch, stream)
}

/// Set stream buffering mode.
///
/// Stub: Our FILE* streams are unbuffered (direct fd I/O), so
/// this accepts all parameters but doesn't change behavior.
///
/// Returns 0 (success) always.
#[unsafe(no_mangle)]
pub extern "C" fn setvbuf(
    _stream: *mut u8,
    _buf: *mut u8,
    _mode: i32,
    _size: usize,
) -> i32 {
    0 // Silently accept — we don't buffer.
}

/// Set stream buffering (simplified version of setvbuf).
///
/// Stub: accepts but doesn't change behavior.
#[unsafe(no_mangle)]
pub extern "C" fn setbuf(_stream: *mut u8, _buf: *mut u8) {
    // No-op: we don't buffer.
}

/// Buffering mode constants.
///
/// We don't actually use these since our I/O is unbuffered,
/// but programs reference them.
#[unsafe(no_mangle)]
pub static _IONBF: i32 = 2;
#[unsafe(no_mangle)]
pub static _IOLBF: i32 = 1;
#[unsafe(no_mangle)]
pub static _IOFBF: i32 = 0;

/// Size constant for setvbuf.
#[unsafe(no_mangle)]
pub static BUFSIZ: i32 = 8192;

/// Open a process pipe.
///
/// Stub: returns null.  Proper implementation requires fork+exec
/// or posix_spawn with pipe redirection.
#[unsafe(no_mangle)]
pub extern "C" fn popen(_command: *const u8, _mode: *const u8) -> *mut u8 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    core::ptr::null_mut()
}

/// Close a process pipe.
///
/// Stub: returns -1.
#[unsafe(no_mangle)]
pub extern "C" fn pclose(_stream: *mut u8) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// FILE* symbols
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// getline / getdelim — POSIX dynamic line reading
// ---------------------------------------------------------------------------

/// Read a delimited record from a stream.
///
/// Reads until `delimiter` is found or EOF.  The buffer `*lineptr`
/// is reallocated via `malloc`/`realloc` as needed.  `*n` holds the
/// current buffer size.
///
/// Returns the number of characters read (including the delimiter),
/// or -1 on error/EOF with no characters read.
///
/// # Safety
///
/// `lineptr`, `n`, and `stream` must be valid pointers.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn getdelim(
    lineptr: *mut *mut u8,
    n: *mut usize,
    delimiter: i32,
    stream: *mut u8,
) -> isize {
    if lineptr.is_null() || n.is_null() || stream.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    let fd = stream as usize as i32;
    let mut buf = unsafe { *lineptr };
    let mut cap = unsafe { *n };
    let mut pos: usize = 0;

    // Ensure initial allocation.
    if buf.is_null() || cap == 0 {
        cap = 128;
        buf = crate::malloc::malloc(cap).cast::<u8>();
        if buf.is_null() {
            crate::errno::set_errno(crate::errno::ENOMEM);
            return -1;
        }
        unsafe {
            *lineptr = buf;
            *n = cap;
        }
    }

    loop {
        let c = fgetc_fd(fd);
        if c == EOF {
            if pos == 0 {
                return -1; // EOF with no data.
            }
            break;
        }

        // Grow buffer if needed (leave room for null terminator).
        if pos >= cap.wrapping_sub(1) {
            let new_cap = cap.wrapping_mul(2);
            // SAFETY: realloc is unsafe extern "C".
            let new_buf = unsafe {
                crate::malloc::realloc(buf.cast::<u8>(), new_cap)
            };
            if new_buf.is_null() {
                crate::errno::set_errno(crate::errno::ENOMEM);
                return -1;
            }
            buf = new_buf.cast::<u8>();
            cap = new_cap;
            unsafe {
                *lineptr = buf;
                *n = cap;
            }
        }

        // SAFETY: pos < cap-1, buf is valid for cap bytes.
        unsafe { *buf.add(pos) = c as u8; }
        pos = pos.wrapping_add(1);

        if c == delimiter {
            break;
        }
    }

    // Null-terminate.
    unsafe { *buf.add(pos) = 0; }
    pos as isize
}

/// Read a line from a stream (up to and including newline).
///
/// Equivalent to `getdelim(lineptr, n, '\n', stream)`.
///
/// # Safety
///
/// `lineptr`, `n`, and `stream` must be valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getline(
    lineptr: *mut *mut u8,
    n: *mut usize,
    stream: *mut u8,
) -> isize {
    unsafe { getdelim(lineptr, n, i32::from(b'\n'), stream) }
}

/// Read a single character from a raw fd (internal helper for getdelim).
///
/// Returns EOF (-1) on end-of-file or error.
fn fgetc_fd(fd: i32) -> i32 {
    let mut byte: u8 = 0;
    let n = crate::file::read(fd, &raw mut byte, 1);
    if n <= 0 { EOF } else { i32::from(byte) }
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Convert a fopen mode string to open flags.
///
/// Returns -1 if the mode is invalid.
fn mode_to_flags(mode: *const u8) -> i32 {
    // SAFETY: Caller guarantees mode is a valid C string.
    let m0 = unsafe { *mode };
    let m1 = unsafe { *mode.add(1) };

    // Check for '+' in position 1 or 2 (after optional 'b').
    let has_plus = m1 == b'+' || (m1 == b'b' && unsafe { *mode.add(2) } == b'+')
        || (m1 != 0 && unsafe { *mode.add(2) } == b'+');

    match (m0, has_plus) {
        (b'r', false) => crate::fcntl::O_RDONLY,
        (b'r', true) => crate::fcntl::O_RDWR,
        (b'w', false) => crate::fcntl::O_WRONLY | crate::fcntl::O_CREAT | crate::fcntl::O_TRUNC,
        (b'w', true) => crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_TRUNC,
        (b'a', false) => crate::fcntl::O_WRONLY | crate::fcntl::O_CREAT | crate::fcntl::O_APPEND,
        (b'a', true) => crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_APPEND,
        _ => -1,
    }
}

// ---------------------------------------------------------------------------
// Thread-safe stdio locking (stubs)
// ---------------------------------------------------------------------------

/// Lock a FILE stream for exclusive thread access.
///
/// Stub: no-op.  Our stdio is not buffered and file operations are
/// already serialised by the kernel.
#[unsafe(no_mangle)]
pub extern "C" fn flockfile(_file: *mut core::ffi::c_void) {
    // No-op: single-threaded per-fd access.
}

/// Try to lock a FILE stream without blocking.
///
/// Stub: always succeeds (returns 0).
#[unsafe(no_mangle)]
pub extern "C" fn ftrylockfile(_file: *mut core::ffi::c_void) -> i32 {
    0
}

/// Unlock a FILE stream.
///
/// Stub: no-op (matches `flockfile`).
#[unsafe(no_mangle)]
pub extern "C" fn funlockfile(_file: *mut core::ffi::c_void) {
    // No-op.
}

/// Non-locking version of `getc`.
///
/// Stub: equivalent to `fgetc` since we don't have internal locks.
#[unsafe(no_mangle)]
pub extern "C" fn getc_unlocked(stream: *mut u8) -> i32 {
    fgetc(stream)
}

/// Non-locking version of `getchar`.
///
/// Reads from stdin (fd 0) without locking.
#[unsafe(no_mangle)]
pub extern "C" fn getchar_unlocked() -> i32 {
    fgetc(core::ptr::null_mut()) // null → fd 0 (stdin) via stream_to_fd
}

/// Non-locking version of `putc`.
///
/// Stub: equivalent to `fputc` since we don't have internal locks.
#[unsafe(no_mangle)]
pub extern "C" fn putc_unlocked(c: i32, stream: *mut u8) -> i32 {
    fputc(c, stream)
}

/// Non-locking version of `putchar`.
///
/// Writes to stdout (fd 1) without locking.
#[unsafe(no_mangle)]
pub extern "C" fn putchar_unlocked(c: i32) -> i32 {
    fputc(c, STDOUT_FILENO as *mut u8)
}
