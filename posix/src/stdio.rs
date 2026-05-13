//! C standard I/O with buffering.
//!
//! Implements `<stdio.h>` with real buffering:
//!
//! - **stdout**: line-buffered (flushes on `'\n'` or buffer full)
//! - **stderr**: unbuffered (every byte goes directly to kernel)
//! - **stdin**: buffered reads (read-ahead into internal buffer)
//! - **fopen'd files**: fully buffered by default
//!
//! Each FILE is backed by a 1 KiB internal buffer.  Three static FILE
//! objects serve stdin/stdout/stderr; a pool of 16 more supports
//! fopen'd files.
//!
//! ## Performance
//!
//! Without buffering, every `putchar` / `fputc` was a separate syscall.
//! A `printf("Hello, world!\n")` that writes 14 bytes now does 1 write
//! syscall (14 bytes buffered, flushed on `'\n'`) instead of 14 individual
//! writes.  For bulk I/O the improvement can be 100× or more.
//!
//! ## Buffer Direction
//!
//! Each FILE buffer serves one direction at a time (read or write).
//! Switching from write to read flushes pending writes; switching from
//! read to write discards any unread buffered data (seeking back if the
//! fd is seekable).

// printf/fprintf/sprintf/snprintf live in the `printf` module (assembly
// trampoline).  They call `write_stream()` from this module for output.

/// File descriptor for stdin.
const STDIN_FD: i32 = 0;
/// File descriptor for stdout.
const STDOUT_FD: i32 = 1;
/// File descriptor for stderr.
const STDERR_FD: i32 = 2;

/// EOF indicator.
pub const EOF: i32 = -1;

/// Internal buffer size per FILE (1 KiB).
const BUF_SIZE: usize = 1024;

// ---------------------------------------------------------------------------
// Buffering mode constants (match the exported _IOFBF / _IOLBF / _IONBF)
// ---------------------------------------------------------------------------

/// Fully buffered — flush only when buffer is full.
const BUF_MODE_FULL: u8 = 0;
/// Line buffered — flush on `'\n'` or when buffer is full.
const BUF_MODE_LINE: u8 = 1;
/// Unbuffered — every write goes directly to the kernel.
const BUF_MODE_NONE: u8 = 2;

// ---------------------------------------------------------------------------
// Buffer direction
// ---------------------------------------------------------------------------

/// Buffer is idle (no pending data in either direction).
const BUF_DIR_IDLE: u8 = 0;
/// Buffer contains read-ahead data.
const BUF_DIR_READ: u8 = 1;
/// Buffer contains pending write data.
const BUF_DIR_WRITE: u8 = 2;

// ---------------------------------------------------------------------------
// FILE status flags (bitfield)
// ---------------------------------------------------------------------------

/// End-of-file has been reached.
const FLAG_EOF: u8 = 1;
/// An I/O error has occurred.
const FLAG_ERR: u8 = 2;

// ---------------------------------------------------------------------------
// FILE struct
// ---------------------------------------------------------------------------

/// Internal FILE structure with I/O buffer.
///
/// C programs treat `FILE` as opaque, so the layout is not part of the
/// ABI.  All access goes through `stdio.h` function calls.
pub struct File {
    /// Underlying kernel file descriptor.
    fd: i32,
    /// I/O buffer.
    buf: [u8; BUF_SIZE],
    /// Current position in the buffer.
    ///
    /// - Write direction: number of bytes buffered (`0..=BUF_SIZE`).
    /// - Read direction: index of next byte to return (`0..=buf_len`).
    buf_pos: usize,
    /// Number of valid bytes in the read buffer.
    ///
    /// Only meaningful when `buf_dir == BUF_DIR_READ`.
    buf_len: usize,
    /// Current buffer direction (`BUF_DIR_IDLE`, `_READ`, or `_WRITE`).
    buf_dir: u8,
    /// Buffering mode (`BUF_MODE_FULL`, `_LINE`, or `_NONE`).
    buf_mode: u8,
    /// Status flags (`FLAG_EOF`, `FLAG_ERR`).
    flags: u8,
    /// Pushed-back byte from `ungetc` (−1 = none).
    ungetc_byte: i16,
}

impl File {
    /// Create a new FILE with the given fd and buffering mode.
    const fn new(fd: i32, mode: u8) -> Self {
        Self {
            fd,
            buf: [0u8; BUF_SIZE],
            buf_pos: 0,
            buf_len: 0,
            buf_dir: BUF_DIR_IDLE,
            buf_mode: mode,
            flags: 0,
            ungetc_byte: -1,
        }
    }
}

// ---------------------------------------------------------------------------
// Standard stream statics
// ---------------------------------------------------------------------------

/// stdin FILE: line-buffered reads.
static mut STDIN_FILE: File = File::new(STDIN_FD, BUF_MODE_LINE);
/// stdout FILE: line-buffered writes.
static mut STDOUT_FILE: File = File::new(STDOUT_FD, BUF_MODE_LINE);
/// stderr FILE: unbuffered (every byte writes immediately).
static mut STDERR_FILE: File = File::new(STDERR_FD, BUF_MODE_NONE);

// ---------------------------------------------------------------------------
// FILE pool for fopen'd files
// ---------------------------------------------------------------------------

/// Maximum number of concurrently fopen'd files.
///
/// 16 files × ~1 KiB buffer = 16 KiB — acceptable for a POSIX compat layer.
const MAX_OPEN_FILES: usize = 16;

struct FileSlot {
    in_use: bool,
    file: File,
}

impl FileSlot {
    const EMPTY: Self = Self {
        in_use: false,
        file: File::new(-1, BUF_MODE_FULL),
    };
}

static mut FILE_POOL: [FileSlot; MAX_OPEN_FILES] = [const { FileSlot::EMPTY }; MAX_OPEN_FILES];

/// Allocate a FILE from the static pool.
///
/// Returns a raw pointer to an available FILE slot, or null if the pool
/// is exhausted.
fn alloc_file(fd: i32, mode: u8) -> *mut File {
    // SAFETY: Single-threaded access (no threads yet).
    unsafe {
        let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
        let mut i: usize = 0;
        while i < MAX_OPEN_FILES {
            let slot = pool.add(i);
            if !(*slot).in_use {
                (*slot).in_use = true;
                (*slot).file = File::new(fd, mode);
                return core::ptr::addr_of_mut!((*slot).file);
            }
            i = i.wrapping_add(1);
        }
    }
    core::ptr::null_mut()
}

/// Return a FILE to the static pool.
fn free_file(file: *mut File) {
    // SAFETY: Single-threaded access.
    unsafe {
        let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
        let mut i: usize = 0;
        while i < MAX_OPEN_FILES {
            let slot = pool.add(i);
            let slot_file = core::ptr::addr_of_mut!((*slot).file);
            if core::ptr::eq(file, slot_file) {
                (*slot).in_use = false;
                return;
            }
            i = i.wrapping_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Stream sentinel conversion
// ---------------------------------------------------------------------------

/// Sentinel `FILE*` pointer values for standard streams.
///
/// C programs read the global `stdin`/`stdout`/`stderr` symbols which
/// contain these small integers.  `stream_to_file` maps them to the
/// real static FILE objects.  All other `FILE*` values are real pointers
/// into `FILE_POOL` (returned by `fopen` / `fdopen`).
const STDIN_SENTINEL: usize = 0;
const STDOUT_SENTINEL: usize = 1;
const STDERR_SENTINEL: usize = 2;

/// Convert a C `FILE*` to our internal `File` pointer.
///
/// Sentinel values 0/1/2 map to the static stdin/stdout/stderr FILEs.
/// All other values are interpreted as real `File` pointers from `fopen`.
fn stream_to_file(stream: *mut u8) -> *mut File {
    match stream as usize {
        STDIN_SENTINEL => core::ptr::addr_of_mut!(STDIN_FILE),
        STDOUT_SENTINEL => core::ptr::addr_of_mut!(STDOUT_FILE),
        STDERR_SENTINEL => core::ptr::addr_of_mut!(STDERR_FILE),
        _ => stream.cast::<File>(),
    }
}

// ---------------------------------------------------------------------------
// Core buffered I/O — internal helpers
// ---------------------------------------------------------------------------

/// Flush write buffer contents to the underlying fd.
///
/// Returns 0 on success, `EOF` on error.
///
/// # Safety
///
/// `f` must point to a valid, exclusively-accessible `File`.
fn file_flush(f: *mut File) -> i32 {
    // SAFETY: Caller guarantees exclusive access.
    let file = unsafe { &mut *f };

    if file.buf_dir != BUF_DIR_WRITE || file.buf_pos == 0 {
        return 0; // Nothing to flush.
    }

    let mut written: usize = 0;
    while written < file.buf_pos {
        let remaining = file.buf_pos.wrapping_sub(written);
        // SAFETY: written < buf_pos <= BUF_SIZE, so buf[written..] is valid.
        let ret = crate::file::write(
            file.fd,
            unsafe { file.buf.as_ptr().add(written) },
            remaining,
        );
        if ret < 0 {
            file.flags |= FLAG_ERR;
            // Shift unsent data to front of buffer so the next flush
            // retries from where we left off.
            if written > 0 {
                let leftover = file.buf_pos.wrapping_sub(written);
                let mut j: usize = 0;
                while j < leftover {
                    // SAFETY: j < leftover, written+j < buf_pos <= BUF_SIZE.
                    file.buf[j] = file.buf[written.wrapping_add(j)];
                    j = j.wrapping_add(1);
                }
                file.buf_pos = leftover;
            }
            return EOF;
        }
        written = written.wrapping_add(ret as usize);
    }

    file.buf_pos = 0;
    0
}

/// Flush all open streams (for `fflush(NULL)`).
fn flush_all() -> i32 {
    let mut result = 0;

    // SAFETY: Raw pointer access to statics, single-threaded.
    unsafe {
        // Flush stdout (line-buffered, may have pending data).
        if file_flush(core::ptr::addr_of_mut!(STDOUT_FILE)) == EOF {
            result = EOF;
        }
        // stderr is unbuffered, but flush for completeness.
        if file_flush(core::ptr::addr_of_mut!(STDERR_FILE)) == EOF {
            result = EOF;
        }

        // Flush all in-use pool files.
        let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
        let mut i: usize = 0;
        while i < MAX_OPEN_FILES {
            let slot = pool.add(i);
            if (*slot).in_use {
                let slot_file = core::ptr::addr_of_mut!((*slot).file);
                if file_flush(slot_file) == EOF {
                    result = EOF;
                }
            }
            i = i.wrapping_add(1);
        }
    }

    result
}

/// Write bytes through the FILE buffer.
///
/// Handles all three buffering modes.  Returns the number of bytes
/// accepted (always `len` on success), or < 0 on error.
///
/// # Safety
///
/// `f` must point to a valid, exclusively-accessible `File`.
/// `data` must be valid for `len` bytes.
fn file_write(f: *mut File, data: *const u8, len: usize) -> i64 {
    if len == 0 {
        return 0;
    }

    // SAFETY: Caller guarantees exclusive access.
    let file = unsafe { &mut *f };

    // --- Unbuffered: write directly to fd ---
    if file.buf_mode == BUF_MODE_NONE {
        // If we were reading, discard the read buffer.
        if file.buf_dir == BUF_DIR_READ {
            file.buf_pos = 0;
            file.buf_len = 0;
        }
        file.buf_dir = BUF_DIR_WRITE;
        let ret = crate::file::write(file.fd, data, len) as i64;
        if ret < 0 {
            file.flags |= FLAG_ERR;
        }
        return ret;
    }

    // --- Switch from read to write direction ---
    if file.buf_dir == BUF_DIR_READ {
        // Seek back past any unread buffered data so the fd position
        // matches the logical position.
        if file.buf_len > file.buf_pos {
            let unread = file.buf_len.wrapping_sub(file.buf_pos);
            let _ = crate::file::lseek(file.fd, -(unread as i64), 1); // SEEK_CUR
        }
        file.buf_pos = 0;
        file.buf_len = 0;
    }
    file.buf_dir = BUF_DIR_WRITE;

    // --- Line-buffered: add to buffer, flush on '\n' or full ---
    if file.buf_mode == BUF_MODE_LINE {
        let mut src: usize = 0;
        while src < len {
            // Flush if buffer is full.
            if file.buf_pos >= BUF_SIZE {
                if file_flush(f) == EOF {
                    return if src > 0 { src as i64 } else { -1 };
                }
                // Re-borrow after flush (flush only touches the same File).
                let file = unsafe { &mut *f };
                let _ = file; // suppress unused warning — we access through f below
            }

            let file = unsafe { &mut *f };
            // SAFETY: src < len, data is valid; buf_pos < BUF_SIZE.
            let byte = unsafe { *data.add(src) };
            if let Some(slot) = file.buf.get_mut(file.buf_pos) {
                *slot = byte;
            }
            file.buf_pos = file.buf_pos.wrapping_add(1);
            src = src.wrapping_add(1);

            // Flush on newline.
            if byte == b'\n' {
                if file_flush(f) == EOF {
                    return if src > 0 { src as i64 } else { -1 };
                }
            }
        }
        return len as i64;
    }

    // --- Fully buffered: bulk copy, flush when full ---
    let mut src: usize = 0;
    while src < len {
        let file = unsafe { &mut *f };
        let space = BUF_SIZE.saturating_sub(file.buf_pos);
        if space == 0 {
            if file_flush(f) == EOF {
                return if src > 0 { src as i64 } else { -1 };
            }
            continue; // Re-check space after flush.
        }

        let file = unsafe { &mut *f };
        let remaining = len.wrapping_sub(src);
        let chunk = if remaining < space { remaining } else { space };
        // SAFETY: buf_pos + chunk <= BUF_SIZE; data+src valid for chunk bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.add(src),
                file.buf.as_mut_ptr().add(file.buf_pos),
                chunk,
            );
        }
        file.buf_pos = file.buf_pos.wrapping_add(chunk);
        src = src.wrapping_add(chunk);

        if file.buf_pos >= BUF_SIZE {
            if file_flush(f) == EOF {
                return if src > 0 { src as i64 } else { -1 };
            }
        }
    }

    len as i64
}

/// Read one byte from the FILE buffer.
///
/// Returns the byte as `i32`, or `EOF` on error/end-of-file.
///
/// # Safety
///
/// `f` must point to a valid, exclusively-accessible `File`.
fn file_read_byte(f: *mut File) -> i32 {
    // SAFETY: Caller guarantees exclusive access.
    let file = unsafe { &mut *f };

    // Check for ungetc'd byte first.
    if file.ungetc_byte >= 0 {
        let ch = file.ungetc_byte as u8;
        file.ungetc_byte = -1;
        file.flags &= !FLAG_EOF;
        return i32::from(ch);
    }

    // Unbuffered: read directly from fd.
    if file.buf_mode == BUF_MODE_NONE {
        let mut byte: u8 = 0;
        let ret = crate::file::read(file.fd, &raw mut byte, 1);
        if ret <= 0 {
            file.flags |= if ret == 0 { FLAG_EOF } else { FLAG_ERR };
            return EOF;
        }
        return i32::from(byte);
    }

    // Switch from write to read direction (flush first).
    if file.buf_dir == BUF_DIR_WRITE {
        file_flush(f);
        let file = unsafe { &mut *f };
        file.buf_pos = 0;
        file.buf_len = 0;
    }
    let file = unsafe { &mut *f };
    file.buf_dir = BUF_DIR_READ;

    // Serve from buffer if data is available.
    if file.buf_pos < file.buf_len {
        let byte = file.buf.get(file.buf_pos).copied().unwrap_or(0);
        file.buf_pos = file.buf_pos.wrapping_add(1);
        return i32::from(byte);
    }

    // Refill buffer from fd.
    let ret = crate::file::read(file.fd, file.buf.as_mut_ptr(), BUF_SIZE);
    if ret <= 0 {
        file.flags |= if ret == 0 { FLAG_EOF } else { FLAG_ERR };
        return EOF;
    }

    file.buf_len = ret as usize;
    file.buf_pos = 1; // Return byte 0, advance past it.
    i32::from(file.buf.get(0).copied().unwrap_or(0))
}

/// Read up to `len` bytes from the FILE buffer.
///
/// Returns the number of bytes actually read, or < 0 on error.
///
/// # Safety
///
/// `f` must point to a valid, exclusively-accessible `File`.
/// `dst` must be valid for `len` bytes.
fn file_read(f: *mut File, dst: *mut u8, len: usize) -> i64 {
    if len == 0 {
        return 0;
    }

    let file = unsafe { &mut *f };
    let mut total: usize = 0;

    // Handle ungetc'd byte first.
    if file.ungetc_byte >= 0 {
        // SAFETY: dst is valid for len bytes, total == 0 < len.
        unsafe { *dst = file.ungetc_byte as u8; }
        file.ungetc_byte = -1;
        file.flags &= !FLAG_EOF;
        total = 1;
        if len == 1 {
            return 1;
        }
    }

    // Unbuffered: read directly from fd.
    if file.buf_mode == BUF_MODE_NONE {
        let remaining = len.wrapping_sub(total);
        let ret = crate::file::read(file.fd, unsafe { dst.add(total) }, remaining);
        if ret < 0 {
            file.flags |= FLAG_ERR;
            return if total > 0 { total as i64 } else { -1 };
        }
        if ret == 0 {
            file.flags |= FLAG_EOF;
        }
        return total.wrapping_add(ret as usize) as i64;
    }

    // Switch from write to read direction (flush first).
    if file.buf_dir == BUF_DIR_WRITE {
        file_flush(f);
        let file = unsafe { &mut *f };
        file.buf_pos = 0;
        file.buf_len = 0;
    }
    let file = unsafe { &mut *f };
    file.buf_dir = BUF_DIR_READ;

    // Drain any data already in the read buffer.
    if file.buf_pos < file.buf_len {
        let available = file.buf_len.wrapping_sub(file.buf_pos);
        let needed = len.wrapping_sub(total);
        let to_copy = if available < needed { available } else { needed };
        // SAFETY: buf_pos + to_copy <= buf_len <= BUF_SIZE;
        //         dst + total + to_copy <= dst + len (caller guarantee).
        unsafe {
            core::ptr::copy_nonoverlapping(
                file.buf.as_ptr().add(file.buf_pos),
                dst.add(total),
                to_copy,
            );
        }
        file.buf_pos = file.buf_pos.wrapping_add(to_copy);
        total = total.wrapping_add(to_copy);
    }

    // Read more from fd if we still need data.
    while total < len {
        let remaining = len.wrapping_sub(total);

        // For large remaining reads, bypass the buffer entirely.
        if remaining >= BUF_SIZE {
            let ret = crate::file::read(file.fd, unsafe { dst.add(total) }, remaining);
            if ret < 0 {
                file.flags |= FLAG_ERR;
                return if total > 0 { total as i64 } else { -1 };
            }
            if ret == 0 {
                file.flags |= FLAG_EOF;
                break;
            }
            total = total.wrapping_add(ret as usize);
            continue;
        }

        // Refill the internal buffer.
        let ret = crate::file::read(file.fd, file.buf.as_mut_ptr(), BUF_SIZE);
        if ret <= 0 {
            if ret == 0 {
                file.flags |= FLAG_EOF;
            } else {
                file.flags |= FLAG_ERR;
            }
            break;
        }
        file.buf_len = ret as usize;
        file.buf_pos = 0;

        let available = file.buf_len;
        let to_copy = if available < remaining { available } else { remaining };
        // SAFETY: to_copy <= buf_len <= BUF_SIZE; dst+total valid.
        unsafe {
            core::ptr::copy_nonoverlapping(
                file.buf.as_ptr(),
                dst.add(total),
                to_copy,
            );
        }
        file.buf_pos = to_copy;
        total = total.wrapping_add(to_copy);

        // Short read from fd means no more data available right now.
        if (ret as usize) < BUF_SIZE {
            break;
        }
    }

    total as i64
}

// ---------------------------------------------------------------------------
// Public entry point for printf/fprintf (crate-internal)
// ---------------------------------------------------------------------------

/// Write bytes to a stream through the FILE buffer.
///
/// Used by the `printf` module to route formatted output through the
/// buffering layer instead of writing directly to the fd.
///
/// Returns the number of bytes written, or < 0 on error.
pub(crate) fn write_stream(stream: *mut u8, data: *const u8, len: usize) -> i64 {
    let file = stream_to_file(stream);
    file_write(file, data, len)
}

// ---------------------------------------------------------------------------
// Output functions
// ---------------------------------------------------------------------------

/// Write a character to stdout.
///
/// Returns the character written as an unsigned char cast to int,
/// or `EOF` (−1) on error.
#[unsafe(no_mangle)]
pub extern "C" fn putchar(c: i32) -> i32 {
    fputc(c, STDOUT_SENTINEL as *mut u8)
}

/// Write a string to stdout followed by a newline.
///
/// Returns a non-negative value on success, `EOF` (−1) on error.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puts(s: *const u8) -> i32 {
    if s.is_null() {
        return EOF;
    }
    let file = stream_to_file(STDOUT_SENTINEL as *mut u8);
    let len = unsafe { crate::string::strlen(s) };
    if file_write(file, s, len) < 0 {
        return EOF;
    }
    // Write trailing newline (triggers line-buffered flush).
    let nl = b'\n';
    if file_write(file, &raw const nl, 1) < 0 {
        return EOF;
    }
    0
}

/// Write a string to a stream.
///
/// Returns a non-negative value on success, `EOF` (−1) on error.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
/// `stream` must be a valid `FILE*`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fputs(s: *const u8, stream: *mut u8) -> i32 {
    if s.is_null() {
        return EOF;
    }
    let file = stream_to_file(stream);
    let len = unsafe { crate::string::strlen(s) };
    let ret = file_write(file, s, len);
    if ret < 0 { EOF } else { 0 }
}

/// Write a character to a stream.
#[unsafe(no_mangle)]
pub extern "C" fn fputc(c: i32, stream: *mut u8) -> i32 {
    let byte = c as u8;
    let file = stream_to_file(stream);
    let ret = file_write(file, &raw const byte, 1);
    if ret < 0 { EOF } else { i32::from(byte) }
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
    let file = stream_to_file(stream);
    let total = size.saturating_mul(nmemb);
    let ret = file_write(file, ptr, total);
    if ret < 0 {
        0
    } else {
        // size > 0 guaranteed by early return above.
        #[allow(clippy::arithmetic_side_effects)]
        { (ret as usize) / size }
    }
}

// ---------------------------------------------------------------------------
// Input functions
// ---------------------------------------------------------------------------

/// Read a character from a stream.
///
/// Returns the character read as an unsigned char cast to int,
/// or `EOF` (−1) on error or end of file.
#[unsafe(no_mangle)]
pub extern "C" fn fgetc(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);

    // Flush stdout before reading from stdin so interactive prompts
    // appear before the program blocks on input.
    // SAFETY: Single-threaded; STDOUT_FILE access is exclusive.
    unsafe {
        if (*file).fd == STDIN_FD {
            file_flush(core::ptr::addr_of_mut!(STDOUT_FILE));
        }
    }

    file_read_byte(file)
}

/// Read a character from stdin.
#[unsafe(no_mangle)]
pub extern "C" fn getchar() -> i32 {
    fgetc(STDIN_SENTINEL as *mut u8)
}

/// Read a character from a stream (function form of `getc` macro).
#[unsafe(no_mangle)]
pub extern "C" fn getc(stream: *mut u8) -> i32 {
    fgetc(stream)
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
    let file = stream_to_file(stream);
    let total = size.saturating_mul(nmemb);
    let ret = file_read(file, ptr, total);
    if ret <= 0 {
        0
    } else {
        // size > 0 guaranteed by early return above.
        #[allow(clippy::arithmetic_side_effects)]
        { (ret as usize) / size }
    }
}

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

    // POSIX: if size is 1, write NUL and return buf (empty string, no read).
    if size == 1 {
        // SAFETY: buf verified non-null, size >= 1.
        unsafe { *buf = 0; }
        return buf;
    }

    let file = stream_to_file(stream);

    // Flush stdout if reading from stdin.
    // SAFETY: Single-threaded; STDOUT_FILE access is exclusive.
    unsafe {
        if (*file).fd == STDIN_FD {
            file_flush(core::ptr::addr_of_mut!(STDOUT_FILE));
        }
    }

    let max = (size as usize).wrapping_sub(1); // Leave room for null.
    let mut pos: usize = 0;

    while pos < max {
        let ch = file_read_byte(file);
        if ch == EOF {
            break;
        }
        // SAFETY: pos < max < size, so buf.add(pos) is valid.
        unsafe { *buf.add(pos) = ch as u8; }
        pos = pos.wrapping_add(1);
        if ch as u8 == b'\n' {
            break;
        }
    }

    if pos == 0 {
        return core::ptr::null_mut(); // EOF before reading anything.
    }

    // Null-terminate.
    // SAFETY: pos <= max = size-1, so buf.add(pos) is within bounds.
    unsafe { *buf.add(pos) = 0; }

    buf
}

// ---------------------------------------------------------------------------
// File management
// ---------------------------------------------------------------------------

/// Open a file as a stdio stream.
///
/// Returns a `FILE*` on success, null on error with `errno` set.
///
/// Supported modes: `"r"`, `"w"`, `"a"`, `"r+"`, `"w+"`, `"a+"`.
/// The `"b"` suffix is accepted and ignored (binary mode is default).
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

    let file_ptr = alloc_file(fd, BUF_MODE_FULL);
    if file_ptr.is_null() {
        crate::file::close(fd);
        crate::errno::set_errno(crate::errno::EMFILE);
        return core::ptr::null_mut();
    }

    file_ptr.cast::<u8>()
}

/// Associate a stream with an existing file descriptor.
///
/// Returns a `FILE*` on success, null on error.
#[unsafe(no_mangle)]
pub extern "C" fn fdopen(fd: i32, _mode: *const u8) -> *mut u8 {
    if fd < 0 {
        crate::errno::set_errno(crate::errno::EBADF);
        return core::ptr::null_mut();
    }

    // Standard streams use their sentinel values.
    match fd {
        STDIN_FD => return STDIN_SENTINEL as *mut u8,
        STDOUT_FD => return STDOUT_SENTINEL as *mut u8,
        STDERR_FD => return STDERR_SENTINEL as *mut u8,
        _ => {}
    }

    let file_ptr = alloc_file(fd, BUF_MODE_FULL);
    if file_ptr.is_null() {
        crate::errno::set_errno(crate::errno::EMFILE);
        return core::ptr::null_mut();
    }

    file_ptr.cast::<u8>()
}

/// Reopen a stream with a different file or mode.
///
/// If `path` is non-null, closes the old stream and opens the new path.
/// If `path` is null, attempts to change the mode (not fully supported).
///
/// POSIX: "The original stream shall be closed regardless of whether
/// the subsequent open succeeds."  On failure, the stream is closed
/// and NULL is returned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn freopen(
    path: *const u8,
    mode: *const u8,
    stream: *mut u8,
) -> *mut u8 {
    if mode.is_null() {
        return core::ptr::null_mut();
    }

    let file = stream_to_file(stream);
    // SAFETY: file is valid; exclusive access.
    let f = unsafe { &mut *file };

    // Flush any buffered data.
    file_flush(file);

    // Reset buffer state.
    f.buf_pos = 0;
    f.buf_len = 0;
    f.buf_dir = BUF_DIR_IDLE;
    f.flags = 0;
    f.ungetc_byte = -1;

    if path.is_null() {
        // Mode change only — not supported, return same stream.
        return stream;
    }

    let old_fd = f.fd;
    let is_std = old_fd <= STDERR_FD;

    // POSIX: close the old stream BEFORE attempting the new open.
    // Standard streams (0/1/2) are not truly closed — their fd will
    // be reused via dup2 below (or left closed on failure).
    if !is_std {
        crate::file::close(old_fd);
        f.fd = -1; // Mark as closed to prevent double-close.
    }

    let flags = mode_to_flags(mode);
    if flags < 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        // Stream is already closed; release pool slot for non-standard.
        if !is_std {
            free_file(file);
        }
        return core::ptr::null_mut();
    }

    let new_fd = crate::file::open(path, flags, 0o666);
    if new_fd < 0 {
        // Open failed; stream is closed per POSIX.
        if !is_std {
            free_file(file);
        }
        return core::ptr::null_mut();
    }

    // For standard streams, dup2 the new fd onto the old one so the
    // File struct keeps using fd 0/1/2.
    if is_std {
        if new_fd != old_fd {
            let duped = crate::file::dup2(new_fd, old_fd);
            crate::file::close(new_fd);
            if duped < 0 {
                return core::ptr::null_mut();
            }
        }
        // fd unchanged (still 0/1/2).
    } else {
        // Pool file: use the new fd.
        let f = unsafe { &mut *file };
        f.fd = new_fd;
    }

    stream
}

/// Close a stdio stream.
///
/// Flushes buffered data and closes the underlying fd.
/// Returns 0 on success, `EOF` on error.
#[unsafe(no_mangle)]
pub extern "C" fn fclose(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);
    // SAFETY: file is valid; exclusive access.
    let f = unsafe { &mut *file };

    // Flush but don't close standard streams.
    if f.fd <= STDERR_FD {
        return if file_flush(file) == EOF { EOF } else { 0 };
    }

    // Flush buffered writes.
    let flush_result = file_flush(file);

    // Close the fd.
    let close_result = crate::file::close(f.fd);

    // Release the pool slot.
    free_file(file);

    if flush_result == EOF || close_result < 0 { EOF } else { 0 }
}

/// Flush a stream's write buffer.
///
/// If `stream` is null, flushes all open streams (POSIX requirement).
/// Returns 0 on success, `EOF` on error.
#[unsafe(no_mangle)]
pub extern "C" fn fflush(stream: *mut u8) -> i32 {
    // fflush(NULL) flushes all open output streams.
    // Note: null maps to stdin via stream_to_file, so we check
    // the raw pointer value before converting.
    if (stream as usize) == 0 {
        return flush_all();
    }
    let file = stream_to_file(stream);
    file_flush(file)
}

// ---------------------------------------------------------------------------
// Seeking
// ---------------------------------------------------------------------------

/// Seek constants (match POSIX).
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

/// Seek within a stream.
///
/// Flushes write buffers and invalidates read buffers before seeking.
/// For `SEEK_CUR`, adjusts the offset to account for read-ahead data.
///
/// Returns 0 on success, −1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn fseek(stream: *mut u8, offset: i64, whence: i32) -> i32 {
    let file = stream_to_file(stream);
    let f = unsafe { &mut *file };

    // Flush pending writes.
    if f.buf_dir == BUF_DIR_WRITE {
        if file_flush(file) == EOF {
            return -1;
        }
        // Re-borrow after flush.
        let f = unsafe { &mut *file };
        let _ = f;
    }

    let f = unsafe { &mut *file };
    let mut actual_offset = offset;

    // Adjust SEEK_CUR for buffered read-ahead: the fd is positioned
    // past data we buffered but haven't returned to the caller yet.
    if whence == SEEK_CUR && f.buf_dir == BUF_DIR_READ {
        let unread = f.buf_len.wrapping_sub(f.buf_pos) as i64;
        let ungetc_adj = if f.ungetc_byte >= 0 { 1_i64 } else { 0 };
        actual_offset = offset.wrapping_sub(unread).wrapping_sub(ungetc_adj);
    }

    // Invalidate buffer and clear stateful indicators.
    f.buf_pos = 0;
    f.buf_len = 0;
    f.buf_dir = BUF_DIR_IDLE;
    f.ungetc_byte = -1;
    f.flags &= !FLAG_EOF; // Clear EOF on seek (POSIX requirement).

    let ret = crate::file::lseek(f.fd, actual_offset, whence);
    if ret < 0 { -1 } else { 0 }
}

/// Get the current position in a stream.
///
/// Accounts for buffered read-ahead or pending write data so the
/// returned position matches the logical stream position.
///
/// Returns the current offset, or −1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn ftell(stream: *mut u8) -> i64 {
    let file = stream_to_file(stream);
    let f = unsafe { &mut *file };

    let raw_pos = crate::file::lseek(f.fd, 0, SEEK_CUR);
    if raw_pos < 0 {
        return -1;
    }

    match f.buf_dir {
        BUF_DIR_READ => {
            // The fd has been read ahead; logical position is behind.
            let unread = f.buf_len.wrapping_sub(f.buf_pos) as i64;
            let ungetc_adj = if f.ungetc_byte >= 0 { 1_i64 } else { 0 };
            raw_pos.wrapping_sub(unread).wrapping_sub(ungetc_adj)
        }
        BUF_DIR_WRITE => {
            // Buffered writes haven't reached the fd yet; logical
            // position is ahead.
            raw_pos.wrapping_add(f.buf_pos as i64)
        }
        _ => raw_pos,
    }
}

/// Rewind a stream to the beginning.
///
/// Equivalent to `fseek(stream, 0, SEEK_SET)` and clears the error
/// indicator (POSIX requirement).
#[unsafe(no_mangle)]
pub extern "C" fn rewind(stream: *mut u8) {
    let _ = fseek(stream, 0, SEEK_SET);
    // Also clear error indicator (POSIX requirement beyond fseek).
    let file = stream_to_file(stream);
    unsafe { (*file).flags &= !FLAG_ERR; }
}

// ---------------------------------------------------------------------------
// Stream status
// ---------------------------------------------------------------------------

/// Get the file descriptor for a stream.
#[unsafe(no_mangle)]
pub extern "C" fn fileno(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);
    unsafe { (*file).fd }
}

/// Check end-of-file indicator for a stream.
///
/// Returns non-zero if the EOF indicator is set, 0 otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn feof(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);
    if unsafe { (*file).flags } & FLAG_EOF != 0 { 1 } else { 0 }
}

/// Check error indicator for a stream.
///
/// Returns non-zero if the error indicator is set, 0 otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn ferror(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);
    if unsafe { (*file).flags } & FLAG_ERR != 0 { 1 } else { 0 }
}

/// Clear error and EOF indicators for a stream.
#[unsafe(no_mangle)]
pub extern "C" fn clearerr(stream: *mut u8) {
    let file = stream_to_file(stream);
    unsafe { (*file).flags &= !(FLAG_EOF | FLAG_ERR); }
}

// ---------------------------------------------------------------------------
// ungetc
// ---------------------------------------------------------------------------

/// Push a character back onto the input stream.
///
/// Only one byte of pushback is guaranteed per stream (POSIX minimum).
/// Returns the character on success, `EOF` on failure.
#[unsafe(no_mangle)]
pub extern "C" fn ungetc(ch: i32, stream: *mut u8) -> i32 {
    if ch == EOF {
        return EOF;
    }
    let file = stream_to_file(stream);
    let f = unsafe { &mut *file };
    f.ungetc_byte = (ch & 0xFF) as i16;
    f.flags &= !FLAG_EOF; // Clear EOF on ungetc (POSIX requirement).
    ch & 0xFF
}

// ---------------------------------------------------------------------------
// Buffering control
// ---------------------------------------------------------------------------

/// Set stream buffering mode.
///
/// `mode` must be `_IOFBF`, `_IOLBF`, or `_IONBF`.
/// `buf` is accepted but ignored (we always use the internal buffer).
/// `size` is accepted but ignored.
///
/// Returns 0 on success, non-zero on error.
#[unsafe(no_mangle)]
pub extern "C" fn setvbuf(
    stream: *mut u8,
    _buf: *mut u8,
    mode: i32,
    _size: usize,
) -> i32 {
    let file = stream_to_file(stream);
    let f = unsafe { &mut *file };

    // Validate mode.
    let new_mode = match mode {
        0 => BUF_MODE_FULL, // _IOFBF
        1 => BUF_MODE_LINE, // _IOLBF
        2 => BUF_MODE_NONE, // _IONBF
        _ => {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
    };

    // Flush any pending data before changing mode.
    if f.buf_dir == BUF_DIR_WRITE {
        file_flush(file);
        // Re-borrow.
        let f = unsafe { &mut *file };
        f.buf_mode = new_mode;
    } else {
        f.buf_mode = new_mode;
    }

    0
}

/// Set stream buffering (simplified version of `setvbuf`).
///
/// If `buf` is null, sets the stream to unbuffered.
/// If `buf` is non-null, sets to fully buffered (buf is ignored).
#[unsafe(no_mangle)]
pub extern "C" fn setbuf(stream: *mut u8, buf: *mut u8) {
    if buf.is_null() {
        setvbuf(stream, core::ptr::null_mut(), 2 /* _IONBF */, 0);
    } else {
        setvbuf(stream, buf, 0 /* _IOFBF */, BUF_SIZE);
    }
}

/// Set stream buffering with explicit buffer size (BSD extension).
///
/// Like `setbuf`, but allows specifying the buffer size.
/// If `buf` is null, sets the stream to unbuffered.
#[unsafe(no_mangle)]
pub extern "C" fn setbuffer(stream: *mut u8, buf: *mut u8, size: usize) {
    if buf.is_null() {
        setvbuf(stream, core::ptr::null_mut(), 2 /* _IONBF */, 0);
    } else {
        setvbuf(stream, buf, 0 /* _IOFBF */, size);
    }
}

/// Set line-buffered mode for a stream (BSD extension).
///
/// Equivalent to `setvbuf(stream, NULL, _IOLBF, 0)`.
#[unsafe(no_mangle)]
pub extern "C" fn setlinebuf(stream: *mut u8) {
    setvbuf(stream, core::ptr::null_mut(), 1 /* _IOLBF */, 0);
}

/// Buffering mode constants.
///
/// Exported as global symbols so C programs can reference `_IONBF`,
/// `_IOLBF`, `_IOFBF` by name.
#[unsafe(no_mangle)]
pub static _IONBF: i32 = 2;
#[unsafe(no_mangle)]
pub static _IOLBF: i32 = 1;
#[unsafe(no_mangle)]
pub static _IOFBF: i32 = 0;

/// Default buffer size (exposed to programs).
///
/// Programs may use this with `setvbuf`.  We accept but ignore user-provided
/// buffers, always using our internal 1 KiB buffer.
#[unsafe(no_mangle)]
pub static BUFSIZ: i32 = 8192;

// ---------------------------------------------------------------------------
// Error output
// ---------------------------------------------------------------------------

/// Print an error message to stderr.
///
/// If `s` is non-null and non-empty, prints `"s: error_string\n"`.
/// Otherwise just prints `"error_string\n"`.
///
/// Writes directly to fd 2 (stderr is unbuffered, so this is equivalent
/// to going through the File buffer but avoids a dependency cycle with
/// the formatting helpers).
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
        let _ = crate::file::write(STDERR_FD, s, slen);
        let _ = crate::file::write(STDERR_FD, c": ".as_ptr().cast::<u8>(), 2);
    }

    if !msg.is_null() {
        let mlen = unsafe { crate::string::strlen(msg) };
        let _ = crate::file::write(STDERR_FD, msg, mlen);
    }

    let nl = b'\n';
    let _ = crate::file::write(STDERR_FD, &raw const nl, 1);
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

/// Remove a file.
///
/// Wrapper around `unlink`.
#[unsafe(no_mangle)]
pub extern "C" fn remove(path: *const u8) -> i32 {
    crate::file::unlink(path)
}

/// Rename a file.
///
/// Wrapper around `file::rename`.
#[unsafe(no_mangle)]
pub extern "C" fn stdio_rename(old: *const u8, new: *const u8) -> i32 {
    crate::file::rename(old, new)
}

/// Maximum length of a `tmpnam`-generated filename (including null).
pub const L_TMPNAM: usize = 20;

/// Create a temporary filename (not thread-safe).
///
/// Generates a unique filename in `/tmp`.  If `s` is non-null, the
/// result is written there (must be at least `L_TMPNAM` bytes); if
/// null, a static buffer is used (not thread-safe).
///
/// Uses a monotonically increasing counter for uniqueness within a
/// single process.  The name has the form `/tmp/tmp_NNNNNN`.
///
/// Note: `tmpnam` is considered insecure (TOCTOU race between name
/// generation and file creation).  Prefer `mkstemp`.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn tmpnam(s: *mut u8) -> *mut u8 {
    static mut COUNTER: u32 = 0;
    static mut STATIC_BUF: [u8; L_TMPNAM] = [0u8; L_TMPNAM];

    // Increment counter (single-threaded; multi-threaded worst case is
    // duplicate names — caller should use mkstemp anyway).
    // SAFETY: Static mutable access.  Best-effort uniqueness.
    let count = unsafe {
        COUNTER = COUNTER.wrapping_add(1);
        COUNTER
    };

    // Build name: "/tmp/tmp_NNNNNN\0"
    let prefix = b"/tmp/tmp_";
    let mut buf_local = [0u8; L_TMPNAM];
    let mut pos: usize = 0;

    for &byte in prefix {
        if let Some(slot) = buf_local.get_mut(pos) {
            *slot = byte;
        }
        pos = pos.wrapping_add(1);
    }

    // Write counter as 6-digit decimal.
    let digits = [
        b'0'.wrapping_add(((count / 100_000) % 10) as u8),
        b'0'.wrapping_add(((count / 10_000) % 10) as u8),
        b'0'.wrapping_add(((count / 1_000) % 10) as u8),
        b'0'.wrapping_add(((count / 100) % 10) as u8),
        b'0'.wrapping_add(((count / 10) % 10) as u8),
        b'0'.wrapping_add((count % 10) as u8),
    ];
    for &d in &digits {
        if let Some(slot) = buf_local.get_mut(pos) {
            *slot = d;
        }
        pos = pos.wrapping_add(1);
    }

    // Null-terminate.
    if let Some(slot) = buf_local.get_mut(pos) {
        *slot = 0;
    }

    if s.is_null() {
        // SAFETY: Writing to static buffer.  Not thread-safe (matches POSIX).
        unsafe {
            let ptr = &raw mut STATIC_BUF;
            core::ptr::copy_nonoverlapping(
                buf_local.as_ptr(),
                (*ptr).as_mut_ptr(),
                pos.wrapping_add(1),
            );
            (*ptr).as_mut_ptr()
        }
    } else {
        // SAFETY: Caller guarantees s has at least L_TMPNAM bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(
                buf_local.as_ptr(),
                s,
                pos.wrapping_add(1), // Include null terminator.
            );
        }
        s
    }
}

// ---------------------------------------------------------------------------
// popen / pclose stubs
// ---------------------------------------------------------------------------

/// Open a process pipe.
///
/// Stub: returns null.  Proper implementation requires fork+exec or
/// `posix_spawn` with pipe redirection.
#[unsafe(no_mangle)]
pub extern "C" fn popen(_command: *const u8, _mode: *const u8) -> *mut u8 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    core::ptr::null_mut()
}

/// Close a process pipe.
///
/// Stub: returns −1.
#[unsafe(no_mangle)]
pub extern "C" fn pclose(_stream: *mut u8) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// getc / putc aliases
// ---------------------------------------------------------------------------

/// Write a character to a stream (function form of `putc` macro).
#[unsafe(no_mangle)]
pub extern "C" fn putc(ch: i32, stream: *mut u8) -> i32 {
    fputc(ch, stream)
}

// ---------------------------------------------------------------------------
// Thread-safe stdio locking (stubs)
// ---------------------------------------------------------------------------

/// Lock a FILE stream for exclusive thread access.
///
/// Stub: no-op.  Our stdio is single-threaded.
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
/// Equivalent to `fgetc` since we don't have internal locks.
#[unsafe(no_mangle)]
pub extern "C" fn getc_unlocked(stream: *mut u8) -> i32 {
    fgetc(stream)
}

/// Non-locking version of `getchar`.
///
/// Reads from stdin without locking.
#[unsafe(no_mangle)]
pub extern "C" fn getchar_unlocked() -> i32 {
    fgetc(STDIN_SENTINEL as *mut u8)
}

/// Non-locking version of `putc`.
///
/// Equivalent to `fputc` since we don't have internal locks.
#[unsafe(no_mangle)]
pub extern "C" fn putc_unlocked(c: i32, stream: *mut u8) -> i32 {
    fputc(c, stream)
}

/// Non-locking version of `putchar`.
///
/// Writes to stdout without locking.
#[unsafe(no_mangle)]
pub extern "C" fn putchar_unlocked(c: i32) -> i32 {
    fputc(c, STDOUT_SENTINEL as *mut u8)
}

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
/// or −1 on error/EOF with no characters read.
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
        // Read through the FILE buffer instead of direct fd read.
        let c = fgetc(stream);
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

// ---------------------------------------------------------------------------
// FILE* global symbols
// ---------------------------------------------------------------------------

/// Global `FILE*` for standard output.
///
/// C programs access this as `extern FILE *stdout;`.  The value 1
/// is a sentinel that `stream_to_file` maps to `STDOUT_FILE`.
#[unsafe(no_mangle)]
pub static stdout: usize = STDOUT_SENTINEL;

/// Global `FILE*` for standard error.
#[unsafe(no_mangle)]
pub static stderr: usize = STDERR_SENTINEL;

/// Global `FILE*` for standard input.
#[unsafe(no_mangle)]
pub static stdin: usize = STDIN_SENTINEL;

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Convert an `fopen` mode string to `open` flags.
///
/// Returns −1 if the mode is invalid.
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
