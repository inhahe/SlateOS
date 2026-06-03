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
pub(crate) const STDIN_SENTINEL: usize = 0;
pub(crate) const STDOUT_SENTINEL: usize = 1;
pub(crate) const STDERR_SENTINEL: usize = 2;

/// Convert a C `FILE*` to our internal `File` pointer.
///
/// Sentinel values 0/1/2 map to the static stdin/stdout/stderr FILEs.
/// All other values are interpreted as real `File` pointers from `fopen`.
fn stream_to_file(stream: *mut u8) -> *mut File {
    match stream as usize {
        STDIN_SENTINEL => core::ptr::addr_of_mut!(STDIN_FILE),
        STDOUT_SENTINEL => core::ptr::addr_of_mut!(STDOUT_FILE),
        STDERR_SENTINEL => core::ptr::addr_of_mut!(STDERR_FILE),
        // SAFETY: Non-sentinel FILE* values are real `File` pointers returned
        // by fopen, which allocates from FILE_POOL (a static [File; MAX_FILES]).
        // Those slots are naturally aligned to `File`'s alignment (8 bytes).
        // The u8 sentinel encoding is only applied to values 0/1/2; all other
        // pointer values come from addr_of_mut! on File-aligned storage.
        #[allow(clippy::cast_ptr_alignment)]
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
        if ret <= 0 {
            // ret < 0: write error.
            // ret == 0: kernel refused to write any bytes (e.g., full
            //   disk, broken pipe).  Treat as fatal to avoid spinning
            //   in an infinite loop.
            file.flags |= FLAG_ERR;
            // Shift unsent data to front of buffer so the next flush
            // retries from where we left off.
            if written > 0 {
                let leftover = file.buf_pos.wrapping_sub(written);
                let mut j: usize = 0;
                while j < leftover {
                    // SAFETY: j < leftover, written+j < buf_pos <= BUF_SIZE.
                    let src_idx = written.wrapping_add(j);
                    let src = file.buf.get(src_idx).copied().unwrap_or(0);
                    if let Some(slot) = file.buf.get_mut(j) {
                        *slot = src;
                    }
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
            // unread fits in i64 (both buf_len and buf_pos < BUF_SIZE = 4096).
            #[allow(clippy::arithmetic_side_effects)]
            let neg_unread = -(unread as i64);
            let _ = crate::file::lseek(file.fd, neg_unread, 1); // SEEK_CUR
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
            if byte == b'\n' && file_flush(f) == EOF {
                return if src > 0 { src as i64 } else { -1 };
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

        if file.buf_pos >= BUF_SIZE && file_flush(f) == EOF {
            return if src > 0 { src as i64 } else { -1 };
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
    i32::from(file.buf.first().copied().unwrap_or(0))
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
        unsafe {
            *dst = file.ungetc_byte as u8;
        }
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
        let to_copy = if available < needed {
            available
        } else {
            needed
        };
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
        let to_copy = if available < remaining {
            available
        } else {
            remaining
        };
        // SAFETY: to_copy <= buf_len <= BUF_SIZE; dst+total valid.
        unsafe {
            core::ptr::copy_nonoverlapping(file.buf.as_ptr(), dst.add(total), to_copy);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
        {
            (ret as usize) / size
        }
    }
}

// ---------------------------------------------------------------------------
// Input functions
// ---------------------------------------------------------------------------

/// Read a character from a stream.
///
/// Returns the character read as an unsigned char cast to int,
/// or `EOF` (−1) on error or end of file.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getchar() -> i32 {
    fgetc(STDIN_SENTINEL as *mut u8)
}

/// Read a character from a stream (function form of `getc` macro).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fread(ptr: *mut u8, size: usize, nmemb: usize, stream: *mut u8) -> usize {
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
        {
            (ret as usize) / size
        }
    }
}

/// Read a line from a stream.
///
/// Reads at most `size - 1` characters into `buf`, stopping at a
/// newline or EOF.  The newline is included if read.  The string is
/// always null-terminated.
///
/// Returns `buf` on success, null on error or EOF with no data read.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgets(buf: *mut u8, size: i32, stream: *mut u8) -> *mut u8 {
    if buf.is_null() || size <= 0 {
        return core::ptr::null_mut();
    }

    // POSIX: if size is 1, write NUL and return buf (empty string, no read).
    if size == 1 {
        // SAFETY: buf verified non-null, size >= 1.
        unsafe {
            *buf = 0;
        }
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
        unsafe {
            *buf.add(pos) = ch as u8;
        }
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
    unsafe {
        *buf.add(pos) = 0;
    }

    buf
}

// ---------------------------------------------------------------------------
// FORTIFY_SOURCE _chk wrappers for the buffered read functions.
//
// Under _FORTIFY_SOURCE the libc headers redirect fgets()/fread() to these
// when the destination size is known at compile time.  glibc aborts via
// __chk_fail() when the requested size exceeds the object size; we instead
// clamp to the object size so the call can never overrun the buffer.
// ---------------------------------------------------------------------------

/// `__fgets_chk(buf, size, n, stream)` — fortified `fgets`.
///
/// `size` is the destination object size; `n` is the requested count.  The
/// effective limit is `min(size, n)`, so the read never exceeds the buffer.
///
/// # Safety
///
/// `buf` must be valid for `min(size, n)` bytes and `stream` a valid `FILE*`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fgets_chk(buf: *mut u8, size: usize, n: i32, stream: *mut u8) -> *mut u8 {
    if n < 0 {
        return core::ptr::null_mut();
    }
    // Clamp the requested count to the destination object size.
    let limit = size.min(n as usize);
    // fgets takes an i32; saturate to i32::MAX if the clamp is somehow larger.
    let count = if limit > i32::MAX as usize {
        i32::MAX
    } else {
        limit as i32
    };
    fgets(buf, count, stream)
}

/// `__fread_chk(ptr, ptrlen, size, nmemb, stream)` — fortified `fread`.
///
/// `ptrlen` is the destination object size.  glibc aborts when
/// `size * nmemb > ptrlen`; we instead reduce `nmemb` so the total stays
/// within the buffer, then delegate to `fread`.  Returns the element count
/// actually read.
///
/// # Safety
///
/// `ptr` must be valid for `ptrlen` bytes and `stream` a valid `FILE*`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __fread_chk(
    ptr: *mut u8,
    ptrlen: usize,
    size: usize,
    nmemb: usize,
    stream: *mut u8,
) -> usize {
    if size == 0 {
        return 0;
    }
    // Clamp nmemb so size*nmemb never exceeds the destination object.
    let max_nmemb = ptrlen.wrapping_div(size);
    let safe_nmemb = nmemb.min(max_nmemb);
    // SAFETY: caller guarantees `ptr`/`stream`; safe_nmemb is bounded so the
    // total byte count fits in `ptrlen`.
    unsafe { fread(ptr, size, safe_nmemb, stream) }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn freopen(path: *const u8, mode: *const u8, stream: *mut u8) -> *mut u8 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

    if flush_result == EOF || close_result < 0 {
        EOF
    } else {
        0
    }
}

/// Flush a stream's write buffer.
///
/// If `stream` is null, flushes all open streams (POSIX requirement).
/// Returns 0 on success, `EOF` on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
        let ungetc_adj = i64::from(f.ungetc_byte >= 0);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
            let ungetc_adj = i64::from(f.ungetc_byte >= 0);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rewind(stream: *mut u8) {
    let _ = fseek(stream, 0, SEEK_SET);
    // Also clear error indicator (POSIX requirement beyond fseek).
    let file = stream_to_file(stream);
    unsafe {
        (*file).flags &= !FLAG_ERR;
    }
}

// ---------------------------------------------------------------------------
// fseeko / ftello — off_t variants (LP64: same as fseek/ftell)
// ---------------------------------------------------------------------------

/// Seek in a stream using `off_t` offset.
///
/// On LP64 platforms this is identical to `fseek`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fseeko(stream: *mut u8, offset: crate::types::OffT, whence: i32) -> i32 {
    fseek(stream, offset, whence)
}

/// Get stream position as `off_t`.
///
/// On LP64 platforms this is identical to `ftell`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftello(stream: *mut u8) -> crate::types::OffT {
    ftell(stream)
}

/// `fseeko64` — LP64 alias.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fseeko64(stream: *mut u8, offset: crate::types::OffT, whence: i32) -> i32 {
    fseek(stream, offset, whence)
}

/// `ftello64` — LP64 alias.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftello64(stream: *mut u8) -> crate::types::OffT {
    ftell(stream)
}

// ---------------------------------------------------------------------------
// fgetpos / fsetpos
// ---------------------------------------------------------------------------

/// File position type.
///
/// On LP64 Linux this is an opaque struct containing the offset and
/// multibyte conversion state.  We simplify to just the offset since
/// our multibyte state is stateless (UTF-8).
pub type FposT = i64;

/// Store the current position of a stream.
///
/// The stored value can later be passed to `fsetpos` to restore
/// the position.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetpos(stream: *mut u8, pos: *mut FposT) -> i32 {
    if pos.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    let offset = ftell(stream);
    if offset < 0 {
        return -1;
    }
    // SAFETY: pos verified non-null.
    unsafe {
        *pos = offset;
    }
    0
}

/// Restore the position of a stream.
///
/// Restores to a position previously stored by `fgetpos`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsetpos(stream: *mut u8, pos: *const FposT) -> i32 {
    if pos.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: pos verified non-null.
    let offset = unsafe { *pos };
    fseek(stream, offset, SEEK_SET)
}

/// `fgetpos64` — LP64 alias.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetpos64(stream: *mut u8, pos: *mut FposT) -> i32 {
    fgetpos(stream, pos)
}

/// `fsetpos64` — LP64 alias.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsetpos64(stream: *mut u8, pos: *const FposT) -> i32 {
    fsetpos(stream, pos)
}

// ---------------------------------------------------------------------------
// Stream status
// ---------------------------------------------------------------------------

/// Get the file descriptor for a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fileno(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);
    unsafe { (*file).fd }
}

/// Check end-of-file indicator for a stream.
///
/// Returns non-zero if the EOF indicator is set, 0 otherwise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn feof(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);
    i32::from(unsafe { (*file).flags } & FLAG_EOF != 0)
}

/// Check error indicator for a stream.
///
/// Returns non-zero if the error indicator is set, 0 otherwise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ferror(stream: *mut u8) -> i32 {
    let file = stream_to_file(stream);
    i32::from(unsafe { (*file).flags } & FLAG_ERR != 0)
}

/// Clear error and EOF indicators for a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clearerr(stream: *mut u8) {
    let file = stream_to_file(stream);
    unsafe {
        (*file).flags &= !(FLAG_EOF | FLAG_ERR);
    }
}

// ---------------------------------------------------------------------------
// ungetc
// ---------------------------------------------------------------------------

/// Push a character back onto the input stream.
///
/// Only one byte of pushback is guaranteed per stream (POSIX minimum).
/// Returns the character on success, `EOF` on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setvbuf(stream: *mut u8, _buf: *mut u8, mode: i32, _size: usize) -> i32 {
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

    // Flush pending writes / discard buffered reads before changing mode.
    if f.buf_dir == BUF_DIR_WRITE {
        file_flush(file);
        // Re-borrow after file_flush (which takes *mut File).
        let f = unsafe { &mut *file };
        f.buf_mode = new_mode;
    } else {
        // Discard any buffered read data — switching modes invalidates
        // the current buffer contents.  Without this, a mode switch
        // (e.g., fully-buffered → unbuffered) could serve stale data
        // from the old buffer on the next read.
        f.buf_pos = 0;
        f.buf_len = 0;
        f.buf_dir = BUF_DIR_IDLE;
        f.buf_mode = new_mode;
    }

    0
}

/// Set stream buffering (simplified version of `setvbuf`).
///
/// If `buf` is null, sets the stream to unbuffered.
/// If `buf` is non-null, sets to fully buffered (buf is ignored).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setlinebuf(stream: *mut u8) {
    setvbuf(stream, core::ptr::null_mut(), 1 /* _IOLBF */, 0);
}

/// Buffering mode constants.
///
/// Exported as global symbols so C programs can reference `_IONBF`,
/// `_IOLBF`, `_IOFBF` by name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static _IONBF: i32 = 2;
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static _IOLBF: i32 = 1;
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static _IOFBF: i32 = 0;

/// Default buffer size (exposed to programs).
///
/// Programs may use this with `setvbuf`.  We accept but ignore user-provided
/// buffers, always using our internal 1 KiB buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remove(path: *const u8) -> i32 {
    crate::file::unlink(path)
}

/// Rename a file.
///
/// Wrapper around `file::rename`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn stdio_rename(old: *const u8, new: *const u8) -> i32 {
    crate::file::rename(old, new)
}

/// Maximum length of a filename (including null).
///
/// glibc defines this as 4096.  Used by programs that allocate
/// `char path[FILENAME_MAX]` buffers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static FILENAME_MAX: i32 = 4096;

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn tmpnam(s: *mut u8) -> *mut u8 {
    static mut COUNTER: u32 = 0;
    static mut STATIC_BUF: [u8; L_TMPNAM] = [0u8; L_TMPNAM];

    // Increment counter (single-threaded; multi-threaded worst case is
    // duplicate names — caller should use mkstemp anyway).
    // SAFETY: Static mutable access.  Best-effort uniqueness.
    let count = unsafe {
        let ptr = core::ptr::addr_of_mut!(COUNTER);
        let c = ptr.read().wrapping_add(1);
        ptr.write(c);
        c
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
// popen / pclose
// ---------------------------------------------------------------------------

/// Maximum concurrent popen'd streams.
const MAX_POPEN: usize = 8;

/// Tracks a popen'd stream's child process for pclose.
struct PopenEntry {
    /// The FILE pointer returned by popen (used as key for lookup).
    stream: *mut u8,
    /// The child process PID (for waitpid in pclose).
    child_pid: i32,
}

/// Table of active popen streams.
///
/// When popen succeeds, it records the FILE* and child PID here.
/// pclose looks up the FILE*, calls waitpid, and removes the entry.
static mut POPEN_TABLE: [Option<PopenEntry>; MAX_POPEN] = [const { None }; MAX_POPEN];

/// Record a popen stream for later pclose.
fn popen_register(stream: *mut u8, child_pid: i32) -> bool {
    // SAFETY: Single-threaded access.
    unsafe {
        let table = core::ptr::addr_of_mut!(POPEN_TABLE);
        for slot in &mut (*table) {
            if slot.is_none() {
                *slot = Some(PopenEntry { stream, child_pid });
                return true;
            }
        }
    }
    false
}

/// Look up and remove a popen stream, returning the child PID.
fn popen_unregister(stream: *mut u8) -> Option<i32> {
    // SAFETY: Single-threaded access.
    unsafe {
        let table = core::ptr::addr_of_mut!(POPEN_TABLE);
        for slot in &mut (*table) {
            if let Some(entry) = slot
                && core::ptr::eq(entry.stream, stream) {
                    let pid = entry.child_pid;
                    *slot = None;
                    return Some(pid);
                }
        }
    }
    None
}

/// Open a process pipe.
///
/// Creates a pipe, spawns `/bin/sh -c <command>` via `posix_spawnp`,
/// and returns a `FILE*` connected to the child's stdin (mode "w")
/// or stdout (mode "r").
///
/// The returned stream must be closed with `pclose()`, not `fclose()`.
///
/// # Limitations
///
/// - Kernel pipe handles lack ref-counting, so the pipe close from the
///   child side may affect the parent.  In practice this works because
///   the child's end is closed via process exit (zombie cleanup).
/// - Only "r" and "w" modes are supported (not "e" for O_CLOEXEC).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn popen(command: *const u8, mode: *const u8) -> *mut u8 {
    use crate::spawn::{
        PosixSpawnFileActionsT, posix_spawn_file_actions_addclose,
        posix_spawn_file_actions_adddup2, posix_spawn_file_actions_init, posix_spawnp,
    };
    use crate::types::PidT;

    if command.is_null() || mode.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    // Parse mode: "r" or "w".
    // SAFETY: mode is non-null, a valid C string per caller contract.
    let mode_byte = unsafe { *mode };
    let is_read = mode_byte == b'r';
    let is_write = mode_byte == b'w';
    if !is_read && !is_write {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    // Create a pipe.  pipefd[0] = read end, pipefd[1] = write end.
    let mut pipefd = [0i32; 2];
    if crate::pipe::pipe(pipefd.as_mut_ptr()) < 0 {
        return core::ptr::null_mut(); // errno already set by pipe().
    }

    // Decide which end belongs to the parent and child.
    // Mode "r": child writes (stdout → pipe write end), parent reads.
    // Mode "w": child reads (stdin → pipe read end), parent writes.
    let (parent_fd, child_fd, child_stdio_fd) = if is_read {
        (pipefd[0], pipefd[1], 1) // Parent reads, child writes to fd 1 (stdout).
    } else {
        (pipefd[1], pipefd[0], 0) // Parent writes, child reads from fd 0 (stdin).
    };

    // Build file_actions for the child:
    // 1. Close the parent's end of the pipe.
    // 2. Dup the child's end to the target stdio fd (0 or 1).
    // 3. Close the original child fd if it differs from the target.
    let mut file_actions: PosixSpawnFileActionsT = unsafe { core::mem::zeroed() };
    posix_spawn_file_actions_init(&raw mut file_actions);
    posix_spawn_file_actions_addclose(&raw mut file_actions, parent_fd);
    if child_fd != child_stdio_fd {
        posix_spawn_file_actions_adddup2(&raw mut file_actions, child_fd, child_stdio_fd);
        posix_spawn_file_actions_addclose(&raw mut file_actions, child_fd);
    }

    // Build argv: ["/bin/sh", "-c", command, NULL].
    let sh = b"/bin/sh\0";
    let dash_c = b"-c\0";
    let argv: [*const u8; 4] = [sh.as_ptr(), dash_c.as_ptr(), command, core::ptr::null()];

    // Spawn the child.
    let mut child_pid: PidT = 0;
    let spawn_ret = posix_spawnp(
        &raw mut child_pid,
        sh.as_ptr(),
        &raw const file_actions,
        core::ptr::null(),
        argv.as_ptr(),
        core::ptr::null(), // Inherit parent's environment.
    );

    // Close the child's end of the pipe in the parent — the child
    // has its own copy (via fd_map inheritance).
    crate::file::close(child_fd);

    if spawn_ret != 0 {
        // Spawn failed — close parent's end too and return null.
        crate::file::close(parent_fd);
        crate::errno::set_errno(spawn_ret);
        return core::ptr::null_mut();
    }

    // Wrap the parent's end in a FILE*.
    let stream = fdopen(
        parent_fd,
        if is_read {
            b"r\0".as_ptr()
        } else {
            b"w\0".as_ptr()
        },
    );
    if stream.is_null() {
        crate::file::close(parent_fd);
        // Can't easily kill the child here, but it will get a broken pipe.
        return core::ptr::null_mut();
    }

    // Register for pclose.
    if !popen_register(stream, child_pid) {
        // Table full — clean up.
        fclose(stream);
        crate::errno::set_errno(crate::errno::EMFILE);
        return core::ptr::null_mut();
    }

    stream
}

/// Close a process pipe opened by `popen()`.
///
/// Closes the stream, then waits for the child process to exit.
/// Returns the child's exit status (as from `waitpid`), or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pclose(stream: *mut u8) -> i32 {
    if stream.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // Look up the child PID.
    let Some(child_pid) = popen_unregister(stream) else {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    };

    // Close the stream (flushes + closes the underlying fd).
    fclose(stream);

    // Wait for the child to exit.
    let mut status: i32 = 0;
    let ret = crate::process::waitpid(child_pid, &raw mut status, 0);
    if ret < 0 {
        return -1; // errno already set by waitpid.
    }

    status
}

// ---------------------------------------------------------------------------
// getc / putc aliases
// ---------------------------------------------------------------------------

/// Write a character to a stream (function form of `putc` macro).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putc(ch: i32, stream: *mut u8) -> i32 {
    fputc(ch, stream)
}

// ---------------------------------------------------------------------------
// Thread-safe stdio locking (stubs)
// ---------------------------------------------------------------------------

/// Lock a FILE stream for exclusive thread access.
///
/// Stub: no-op.  Our stdio is single-threaded.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn flockfile(_file: *mut core::ffi::c_void) {
    // No-op: single-threaded per-fd access.
}

/// Try to lock a FILE stream without blocking.
///
/// Stub: always succeeds (returns 0).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftrylockfile(_file: *mut core::ffi::c_void) -> i32 {
    0
}

/// Unlock a FILE stream.
///
/// Stub: no-op (matches `flockfile`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn funlockfile(_file: *mut core::ffi::c_void) {
    // No-op.
}

/// Non-locking version of `getc`.
///
/// Equivalent to `fgetc` since we don't have internal locks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getc_unlocked(stream: *mut u8) -> i32 {
    fgetc(stream)
}

/// Non-locking version of `getchar`.
///
/// Reads from stdin without locking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getchar_unlocked() -> i32 {
    fgetc(STDIN_SENTINEL as *mut u8)
}

/// Non-locking version of `putc`.
///
/// Equivalent to `fputc` since we don't have internal locks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putc_unlocked(c: i32, stream: *mut u8) -> i32 {
    fputc(c, stream)
}

/// Non-locking version of `putchar`.
///
/// Writes to stdout without locking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn getdelim(
    lineptr: *mut *mut u8,
    n: *mut usize,
    delimiter: i32,
    stream: *mut u8,
) -> isize {
    if lineptr.is_null() || n.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // Note: we don't check stream.is_null() because our stdin sentinel
    // IS null (STDIN_SENTINEL = 0).  stream_to_file() maps 0 → stdin.

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
            let Some(new_cap) = cap.checked_mul(2) else {
                crate::errno::set_errno(crate::errno::ENOMEM);
                return -1;
            };
            // SAFETY: realloc is unsafe extern "C".
            let new_buf = unsafe { crate::malloc::realloc(buf.cast::<u8>(), new_cap) };
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
        unsafe {
            *buf.add(pos) = c as u8;
        }
        pos = pos.wrapping_add(1);

        if c == delimiter {
            break;
        }
    }

    // Null-terminate.
    unsafe {
        *buf.add(pos) = 0;
    }
    pos as isize
}

/// Read a line from a stream (up to and including newline).
///
/// Equivalent to `getdelim(lineptr, n, '\n', stream)`.
///
/// # Safety
///
/// `lineptr`, `n`, and `stream` must be valid pointers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getline(lineptr: *mut *mut u8, n: *mut usize, stream: *mut u8) -> isize {
    unsafe { getdelim(lineptr, n, i32::from(b'\n'), stream) }
}

// ---------------------------------------------------------------------------
// FILE* global symbols
// ---------------------------------------------------------------------------

/// Global `FILE*` for standard output.
///
/// C programs access this as `extern FILE *stdout;`.  The value 1
/// is a sentinel that `stream_to_file` maps to `STDOUT_FILE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static stdout: usize = STDOUT_SENTINEL;

/// Global `FILE*` for standard error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static stderr: usize = STDERR_SENTINEL;

/// Global `FILE*` for standard input.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static stdin: usize = STDIN_SENTINEL;

// glibc internal FILE* aliases — some programs reference these instead
// of the standard names.
/// glibc alias for `stdin`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _IO_stdin_: usize = STDIN_SENTINEL;

/// glibc alias for `stdout`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _IO_stdout_: usize = STDOUT_SENTINEL;

/// glibc alias for `stderr`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _IO_stderr_: usize = STDERR_SENTINEL;

// ---------------------------------------------------------------------------
// LP64 aliases — fopen64
// ---------------------------------------------------------------------------

/// `fopen64` — alias for `fopen` on LP64.
///
/// # Safety
///
/// Same requirements as `fopen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fopen64(path: *const u8, mode: *const u8) -> *mut u8 {
    unsafe { fopen(path, mode) }
}

/// `freopen64` — alias for `freopen` on LP64.
///
/// # Safety
///
/// Same requirements as `freopen`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn freopen64(path: *const u8, mode: *const u8, stream: *mut u8) -> *mut u8 {
    unsafe { freopen(path, mode, stream) }
}

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
    let has_plus = m1 == b'+'
        || (m1 == b'b' && unsafe { *mode.add(2) } == b'+')
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_eof_value() {
        assert_eq!(EOF, -1, "EOF must be -1");
    }

    #[test]
    fn test_buffering_mode_constants() {
        // _IOFBF = 0, _IOLBF = 1, _IONBF = 2 (POSIX/glibc values).
        assert_eq!(BUF_MODE_FULL, 0);
        assert_eq!(BUF_MODE_LINE, 1);
        assert_eq!(BUF_MODE_NONE, 2);
    }

    #[test]
    fn test_buf_size() {
        // Internal buffer should be at least 256 bytes for reasonable perf.
        assert!(BUF_SIZE >= 256, "BUF_SIZE too small: {BUF_SIZE}");
    }

    // -----------------------------------------------------------------------
    // File struct layout
    // -----------------------------------------------------------------------

    #[test]
    fn test_file_struct_default_state() {
        let f = File::new(42, BUF_MODE_FULL);
        assert_eq!(f.fd, 42);
        assert_eq!(f.buf_pos, 0);
        assert_eq!(f.buf_len, 0);
        assert_eq!(f.buf_dir, BUF_DIR_IDLE);
        assert_eq!(f.buf_mode, BUF_MODE_FULL);
        assert_eq!(f.flags, 0);
        assert_eq!(f.ungetc_byte, -1, "no pushed-back byte initially");
    }

    #[test]
    fn test_standard_stream_modes() {
        // stdin: line-buffered.
        let f_in = File::new(STDIN_FD, BUF_MODE_LINE);
        assert_eq!(f_in.fd, 0);
        assert_eq!(f_in.buf_mode, BUF_MODE_LINE);

        // stdout: line-buffered.
        let f_out = File::new(STDOUT_FD, BUF_MODE_LINE);
        assert_eq!(f_out.fd, 1);
        assert_eq!(f_out.buf_mode, BUF_MODE_LINE);

        // stderr: unbuffered.
        let f_err = File::new(STDERR_FD, BUF_MODE_NONE);
        assert_eq!(f_err.fd, 2);
        assert_eq!(f_err.buf_mode, BUF_MODE_NONE);
    }

    // -----------------------------------------------------------------------
    // mode_to_flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_mode_to_flags_read() {
        let flags = mode_to_flags(b"r\0".as_ptr());
        assert_eq!(flags, crate::fcntl::O_RDONLY);
    }

    #[test]
    fn test_mode_to_flags_write() {
        let flags = mode_to_flags(b"w\0".as_ptr());
        assert_eq!(
            flags,
            crate::fcntl::O_WRONLY | crate::fcntl::O_CREAT | crate::fcntl::O_TRUNC
        );
    }

    #[test]
    fn test_mode_to_flags_append() {
        let flags = mode_to_flags(b"a\0".as_ptr());
        assert_eq!(
            flags,
            crate::fcntl::O_WRONLY | crate::fcntl::O_CREAT | crate::fcntl::O_APPEND
        );
    }

    #[test]
    fn test_mode_to_flags_read_plus() {
        assert_eq!(mode_to_flags(b"r+\0".as_ptr()), crate::fcntl::O_RDWR);
    }

    #[test]
    fn test_mode_to_flags_write_plus() {
        assert_eq!(
            mode_to_flags(b"w+\0".as_ptr()),
            crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_TRUNC
        );
    }

    #[test]
    fn test_mode_to_flags_append_plus() {
        assert_eq!(
            mode_to_flags(b"a+\0".as_ptr()),
            crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_APPEND
        );
    }

    #[test]
    fn test_mode_to_flags_binary_variants() {
        // "rb" = read binary (binary flag is a no-op, still O_RDONLY).
        assert_eq!(mode_to_flags(b"rb\0".as_ptr()), crate::fcntl::O_RDONLY);
        // "wb" = write binary.
        assert_eq!(
            mode_to_flags(b"wb\0".as_ptr()),
            crate::fcntl::O_WRONLY | crate::fcntl::O_CREAT | crate::fcntl::O_TRUNC
        );
        // "ab" = append binary.
        assert_eq!(
            mode_to_flags(b"ab\0".as_ptr()),
            crate::fcntl::O_WRONLY | crate::fcntl::O_CREAT | crate::fcntl::O_APPEND
        );
    }

    #[test]
    fn test_mode_to_flags_binary_plus() {
        // "rb+" and "r+b" should both yield O_RDWR.
        assert_eq!(mode_to_flags(b"rb+\0".as_ptr()), crate::fcntl::O_RDWR);
        assert_eq!(mode_to_flags(b"r+b\0".as_ptr()), crate::fcntl::O_RDWR);
        // "wb+" and "w+b" should both yield O_RDWR|O_CREAT|O_TRUNC.
        let expected_w = crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_TRUNC;
        assert_eq!(mode_to_flags(b"wb+\0".as_ptr()), expected_w);
        assert_eq!(mode_to_flags(b"w+b\0".as_ptr()), expected_w);
        // "ab+" and "a+b".
        let expected_a = crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_APPEND;
        assert_eq!(mode_to_flags(b"ab+\0".as_ptr()), expected_a);
        assert_eq!(mode_to_flags(b"a+b\0".as_ptr()), expected_a);
    }

    #[test]
    fn test_mode_to_flags_invalid() {
        assert_eq!(mode_to_flags(b"x\0".as_ptr()), -1);
        assert_eq!(mode_to_flags(b"z\0".as_ptr()), -1);
    }

    // -----------------------------------------------------------------------
    // Sentinel system
    // -----------------------------------------------------------------------

    #[test]
    fn test_sentinel_values() {
        assert_eq!(STDIN_SENTINEL, 0);
        assert_eq!(STDOUT_SENTINEL, 1);
        assert_eq!(STDERR_SENTINEL, 2);
    }

    #[test]
    fn test_stream_to_file_sentinel_stdin() {
        let file = stream_to_file(STDIN_SENTINEL as *mut u8);
        assert!(!file.is_null());
        let f = unsafe { &*file };
        assert_eq!(f.fd, STDIN_FD);
    }

    #[test]
    fn test_stream_to_file_sentinel_stdout() {
        let file = stream_to_file(STDOUT_SENTINEL as *mut u8);
        assert!(!file.is_null());
        let f = unsafe { &*file };
        assert_eq!(f.fd, STDOUT_FD);
    }

    #[test]
    fn test_stream_to_file_sentinel_stderr() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        assert!(!file.is_null());
        let f = unsafe { &*file };
        assert_eq!(f.fd, STDERR_FD);
    }

    // -----------------------------------------------------------------------
    // FILE flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_flag_bits_distinct() {
        assert_eq!(FLAG_EOF & FLAG_ERR, 0, "EOF and ERR flags must not overlap");
    }

    #[test]
    fn test_ungetc_byte_sentinel() {
        // -1 means no byte pushed back.
        let f = File::new(0, BUF_MODE_LINE);
        assert!(f.ungetc_byte < 0, "no pushed byte initially");
    }

    // -----------------------------------------------------------------------
    // File pool
    // -----------------------------------------------------------------------

    #[test]
    fn test_file_slot_empty() {
        let slot = FileSlot::EMPTY;
        assert!(!slot.in_use);
        assert_eq!(slot.file.fd, -1);
    }

    // -----------------------------------------------------------------------
    // popen / pclose infrastructure
    // -----------------------------------------------------------------------

    /// Helper: clear the popen table for test isolation.
    fn reset_popen_table() {
        unsafe {
            let table = core::ptr::addr_of_mut!(POPEN_TABLE);
            for slot in (*table).iter_mut() {
                *slot = None;
            }
        }
    }

    #[test]
    fn test_popen_register_and_unregister() {
        reset_popen_table();
        let fake_stream = 0x1000 as *mut u8;
        assert!(popen_register(fake_stream, 42));
        assert_eq!(popen_unregister(fake_stream), Some(42));
        // Second unregister should return None.
        assert_eq!(popen_unregister(fake_stream), None);
    }

    #[test]
    fn test_popen_register_multiple() {
        reset_popen_table();
        let s1 = 0x1000 as *mut u8;
        let s2 = 0x2000 as *mut u8;
        let s3 = 0x3000 as *mut u8;
        assert!(popen_register(s1, 10));
        assert!(popen_register(s2, 20));
        assert!(popen_register(s3, 30));
        assert_eq!(popen_unregister(s2), Some(20));
        assert_eq!(popen_unregister(s1), Some(10));
        assert_eq!(popen_unregister(s3), Some(30));
    }

    #[test]
    fn test_popen_register_full_table() {
        reset_popen_table();
        // Fill all MAX_POPEN slots.
        for i in 0..MAX_POPEN {
            let fake = ((i + 1) * 0x1000) as *mut u8;
            assert!(popen_register(fake, i as i32));
        }
        // Next register should fail.
        let overflow = 0xFFFF as *mut u8;
        assert!(!popen_register(overflow, 99));
        // Clean up.
        for i in 0..MAX_POPEN {
            let fake = ((i + 1) * 0x1000) as *mut u8;
            popen_unregister(fake);
        }
    }

    #[test]
    fn test_popen_unregister_nonexistent() {
        reset_popen_table();
        let fake = 0xDEAD as *mut u8;
        assert_eq!(popen_unregister(fake), None);
    }

    #[test]
    fn test_popen_null_command() {
        let ret = popen(core::ptr::null(), b"r\0".as_ptr());
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_popen_null_mode() {
        let ret = popen(b"ls\0".as_ptr(), core::ptr::null());
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_popen_invalid_mode() {
        let ret = popen(b"ls\0".as_ptr(), b"x\0".as_ptr());
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pclose_null() {
        assert_eq!(pclose(core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pclose_unknown_stream() {
        reset_popen_table();
        let fake = 0xBEEF as *mut u8;
        assert_eq!(pclose(fake), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_max_popen_constant() {
        assert_eq!(MAX_POPEN, 8);
    }

    // -----------------------------------------------------------------------
    // Exported constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_seek_constants() {
        assert_eq!(SEEK_SET, 0);
        assert_eq!(SEEK_CUR, 1);
        assert_eq!(SEEK_END, 2);
    }

    #[test]
    fn test_ionbf_iolbf_iofbf_values() {
        assert_eq!(_IONBF, 2);
        assert_eq!(_IOLBF, 1);
        assert_eq!(_IOFBF, 0);
    }

    #[test]
    fn test_bufsiz_value() {
        // glibc defines BUFSIZ as 8192.
        assert_eq!(BUFSIZ, 8192);
    }

    #[test]
    fn test_filename_max_value() {
        // glibc defines FILENAME_MAX as 4096.
        assert_eq!(FILENAME_MAX, 4096);
    }

    #[test]
    fn test_l_tmpnam_value() {
        // Must be large enough for "/tmp/tmp_NNNNNN\0" = 16 chars + null.
        assert!(L_TMPNAM >= 17, "L_TMPNAM too small for tmpnam format");
    }

    #[test]
    fn test_fpos_t_is_i64() {
        assert_eq!(
            core::mem::size_of::<FposT>(),
            core::mem::size_of::<i64>(),
            "FposT must be i64-sized"
        );
    }

    #[test]
    fn test_max_open_files() {
        assert_eq!(MAX_OPEN_FILES, 16);
    }

    // -----------------------------------------------------------------------
    // Buffer direction constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_buf_dir_constants_distinct() {
        assert_ne!(BUF_DIR_IDLE, BUF_DIR_READ);
        assert_ne!(BUF_DIR_IDLE, BUF_DIR_WRITE);
        assert_ne!(BUF_DIR_READ, BUF_DIR_WRITE);
    }

    // -----------------------------------------------------------------------
    // Global FILE* symbols
    // -----------------------------------------------------------------------

    #[test]
    fn test_stdout_symbol() {
        assert_eq!(stdout, STDOUT_SENTINEL);
    }

    #[test]
    fn test_stderr_symbol() {
        assert_eq!(stderr, STDERR_SENTINEL);
    }

    #[test]
    fn test_stdin_symbol() {
        assert_eq!(stdin, STDIN_SENTINEL);
    }

    #[test]
    fn test_glibc_stdin_alias() {
        assert_eq!(_IO_stdin_, STDIN_SENTINEL);
    }

    #[test]
    fn test_glibc_stdout_alias() {
        assert_eq!(_IO_stdout_, STDOUT_SENTINEL);
    }

    #[test]
    fn test_glibc_stderr_alias() {
        assert_eq!(_IO_stderr_, STDERR_SENTINEL);
    }

    // -----------------------------------------------------------------------
    // fileno — returns fd for sentinel streams
    // -----------------------------------------------------------------------

    #[test]
    fn test_fileno_stdin() {
        assert_eq!(fileno(STDIN_SENTINEL as *mut u8), STDIN_FD);
    }

    #[test]
    fn test_fileno_stdout() {
        assert_eq!(fileno(STDOUT_SENTINEL as *mut u8), STDOUT_FD);
    }

    #[test]
    fn test_fileno_stderr() {
        assert_eq!(fileno(STDERR_SENTINEL as *mut u8), STDERR_FD);
    }

    // -----------------------------------------------------------------------
    // feof / ferror / clearerr — flag manipulation
    // -----------------------------------------------------------------------

    #[test]
    fn test_feof_initially_zero() {
        // stdout starts with no EOF flag.
        // Reset flag state first to avoid interference from prior tests.
        let file = stream_to_file(STDOUT_SENTINEL as *mut u8);
        unsafe {
            (*file).flags &= !FLAG_EOF;
        }
        assert_eq!(feof(STDOUT_SENTINEL as *mut u8), 0);
    }

    #[test]
    fn test_ferror_initially_zero() {
        let file = stream_to_file(STDOUT_SENTINEL as *mut u8);
        unsafe {
            (*file).flags &= !FLAG_ERR;
        }
        assert_eq!(ferror(STDOUT_SENTINEL as *mut u8), 0);
    }

    #[test]
    fn test_feof_after_setting_flag() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_flags = unsafe { (*file).flags };
        unsafe {
            (*file).flags |= FLAG_EOF;
        }
        assert_ne!(feof(STDERR_SENTINEL as *mut u8), 0);
        // Restore.
        unsafe {
            (*file).flags = old_flags;
        }
    }

    #[test]
    fn test_ferror_after_setting_flag() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_flags = unsafe { (*file).flags };
        unsafe {
            (*file).flags |= FLAG_ERR;
        }
        assert_ne!(ferror(STDERR_SENTINEL as *mut u8), 0);
        // Restore.
        unsafe {
            (*file).flags = old_flags;
        }
    }

    #[test]
    fn test_clearerr_clears_both_flags() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_flags = unsafe { (*file).flags };
        unsafe {
            (*file).flags |= FLAG_EOF | FLAG_ERR;
        }
        clearerr(STDERR_SENTINEL as *mut u8);
        assert_eq!(feof(STDERR_SENTINEL as *mut u8), 0);
        assert_eq!(ferror(STDERR_SENTINEL as *mut u8), 0);
        // Restore.
        unsafe {
            (*file).flags = old_flags;
        }
    }

    // -----------------------------------------------------------------------
    // ungetc — pushback
    // -----------------------------------------------------------------------

    #[test]
    fn test_ungetc_stores_byte() {
        let file = stream_to_file(STDIN_SENTINEL as *mut u8);
        let old_byte = unsafe { (*file).ungetc_byte };
        let old_flags = unsafe { (*file).flags };

        let ret = ungetc(b'X' as i32, STDIN_SENTINEL as *mut u8);
        assert_eq!(ret, b'X' as i32);
        assert_eq!(unsafe { (*file).ungetc_byte }, b'X' as i16);

        // Restore.
        unsafe {
            (*file).ungetc_byte = old_byte;
            (*file).flags = old_flags;
        }
    }

    #[test]
    fn test_ungetc_eof_returns_eof() {
        let ret = ungetc(EOF, STDIN_SENTINEL as *mut u8);
        assert_eq!(ret, EOF, "ungetc(EOF) must return EOF");
    }

    #[test]
    fn test_ungetc_clears_eof_flag() {
        let file = stream_to_file(STDIN_SENTINEL as *mut u8);
        let old_flags = unsafe { (*file).flags };
        let old_byte = unsafe { (*file).ungetc_byte };

        // Set EOF flag.
        unsafe {
            (*file).flags |= FLAG_EOF;
        }
        // Push back a byte — should clear EOF.
        ungetc(b'A' as i32, STDIN_SENTINEL as *mut u8);
        assert_eq!(feof(STDIN_SENTINEL as *mut u8), 0, "ungetc must clear EOF");

        // Restore.
        unsafe {
            (*file).ungetc_byte = old_byte;
            (*file).flags = old_flags;
        }
    }

    #[test]
    fn test_ungetc_masks_to_byte() {
        let file = stream_to_file(STDIN_SENTINEL as *mut u8);
        let old_byte = unsafe { (*file).ungetc_byte };

        // 0x1FF & 0xFF = 0xFF (should store only the low byte).
        let ret = ungetc(0x1FF, STDIN_SENTINEL as *mut u8);
        assert_eq!(ret, 0xFF);
        assert_eq!(unsafe { (*file).ungetc_byte }, 0xFF);

        // Restore.
        unsafe {
            (*file).ungetc_byte = old_byte;
        }
    }

    // -----------------------------------------------------------------------
    // fdopen — error paths and standard streams
    // -----------------------------------------------------------------------

    #[test]
    fn test_fdopen_negative_fd() {
        let ret = fdopen(-1, b"r\0".as_ptr());
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fdopen_stdin_returns_sentinel() {
        let ret = fdopen(STDIN_FD, b"r\0".as_ptr());
        assert_eq!(ret as usize, STDIN_SENTINEL);
    }

    #[test]
    fn test_fdopen_stdout_returns_sentinel() {
        let ret = fdopen(STDOUT_FD, b"w\0".as_ptr());
        assert_eq!(ret as usize, STDOUT_SENTINEL);
    }

    #[test]
    fn test_fdopen_stderr_returns_sentinel() {
        let ret = fdopen(STDERR_FD, b"w\0".as_ptr());
        assert_eq!(ret as usize, STDERR_SENTINEL);
    }

    // -----------------------------------------------------------------------
    // fopen — null argument handling
    // -----------------------------------------------------------------------

    #[test]
    fn test_fopen_null_path() {
        let ret = unsafe { fopen(core::ptr::null(), b"r\0".as_ptr()) };
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fopen_null_mode() {
        let ret = unsafe { fopen(b"/tmp/test\0".as_ptr(), core::ptr::null()) };
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // fgetpos / fsetpos — null pointer errors
    // -----------------------------------------------------------------------

    #[test]
    fn test_fgetpos_null_pos() {
        let ret = fgetpos(STDOUT_SENTINEL as *mut u8, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fsetpos_null_pos() {
        let ret = fsetpos(STDOUT_SENTINEL as *mut u8, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // setvbuf — mode validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_setvbuf_invalid_mode() {
        let ret = setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 99, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_setvbuf_valid_modes() {
        // Each valid mode (0=_IOFBF, 1=_IOLBF, 2=_IONBF) should succeed.
        // Test on stderr (unbuffered) since we can restore its mode.
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };

        for mode in 0..3 {
            let ret = setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), mode, 0);
            assert_eq!(ret, 0, "setvbuf mode {mode} should succeed");
        }

        // Restore stderr's original mode (unbuffered).
        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    // -----------------------------------------------------------------------
    // tmpnam — filename generation
    // -----------------------------------------------------------------------

    #[test]
    fn test_tmpnam_user_buffer() {
        let mut buf = [0u8; L_TMPNAM];
        let ret = tmpnam(buf.as_mut_ptr());
        assert_eq!(ret, buf.as_mut_ptr());
        // Should start with "/tmp/tmp_".
        assert_eq!(&buf[..9], b"/tmp/tmp_");
        // Should be null-terminated within L_TMPNAM.
        let has_null = buf.iter().any(|&b| b == 0);
        assert!(has_null, "tmpnam result must be null-terminated");
    }

    #[test]
    fn test_tmpnam_null_uses_static() {
        let ret1 = tmpnam(core::ptr::null_mut());
        assert!(!ret1.is_null());
        // Should start with "/tmp/tmp_".
        let prefix = unsafe { core::slice::from_raw_parts(ret1, 9) };
        assert_eq!(prefix, b"/tmp/tmp_");
    }

    #[test]
    fn test_tmpnam_generates_unique_names() {
        let mut buf1 = [0u8; L_TMPNAM];
        let mut buf2 = [0u8; L_TMPNAM];
        tmpnam(buf1.as_mut_ptr());
        tmpnam(buf2.as_mut_ptr());
        // Two consecutive calls should produce different names.
        assert_ne!(buf1, buf2, "tmpnam must generate unique names");
    }

    // -----------------------------------------------------------------------
    // getdelim / getline — null-argument errors
    // -----------------------------------------------------------------------

    #[test]
    fn test_getdelim_null_lineptr() {
        let mut n: usize = 0;
        let ret = unsafe {
            getdelim(
                core::ptr::null_mut(),
                &raw mut n,
                b'\n' as i32,
                STDIN_SENTINEL as *mut u8,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdelim_null_n() {
        let mut lineptr: *mut u8 = core::ptr::null_mut();
        let ret = unsafe {
            getdelim(
                &raw mut lineptr,
                core::ptr::null_mut(),
                b'\n' as i32,
                STDIN_SENTINEL as *mut u8,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // puts — null pointer
    // -----------------------------------------------------------------------

    #[test]
    fn test_puts_null() {
        let ret = unsafe { puts(core::ptr::null()) };
        assert_eq!(ret, EOF);
    }

    // -----------------------------------------------------------------------
    // fputs — null pointer
    // -----------------------------------------------------------------------

    #[test]
    fn test_fputs_null_string() {
        let ret = unsafe { fputs(core::ptr::null(), STDOUT_SENTINEL as *mut u8) };
        assert_eq!(ret, EOF);
    }

    // -----------------------------------------------------------------------
    // fwrite — null/zero args
    // -----------------------------------------------------------------------

    #[test]
    fn test_fwrite_null_ptr() {
        let ret = unsafe { fwrite(core::ptr::null(), 1, 10, STDOUT_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fwrite_zero_size() {
        let data = [1u8; 10];
        let ret = unsafe { fwrite(data.as_ptr(), 0, 10, STDOUT_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fwrite_zero_nmemb() {
        let data = [1u8; 10];
        let ret = unsafe { fwrite(data.as_ptr(), 1, 0, STDOUT_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // fread — null/zero args
    // -----------------------------------------------------------------------

    #[test]
    fn test_fread_null_ptr() {
        let ret = unsafe { fread(core::ptr::null_mut(), 1, 10, STDIN_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fread_zero_size() {
        let mut buf = [0u8; 10];
        let ret = unsafe { fread(buf.as_mut_ptr(), 0, 10, STDIN_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fread_zero_nmemb() {
        let mut buf = [0u8; 10];
        let ret = unsafe { fread(buf.as_mut_ptr(), 1, 0, STDIN_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // fgets — null/zero args
    // -----------------------------------------------------------------------

    #[test]
    fn test_fgets_null_buf() {
        let ret = fgets(core::ptr::null_mut(), 100, STDIN_SENTINEL as *mut u8);
        assert!(ret.is_null());
    }

    #[test]
    fn test_fgets_zero_size() {
        let mut buf = [0u8; 10];
        let ret = fgets(buf.as_mut_ptr(), 0, STDIN_SENTINEL as *mut u8);
        assert!(ret.is_null());
    }

    #[test]
    fn test_fgets_negative_size() {
        let mut buf = [0u8; 10];
        let ret = fgets(buf.as_mut_ptr(), -1, STDIN_SENTINEL as *mut u8);
        assert!(ret.is_null());
    }

    #[test]
    fn test_fgets_size_one() {
        // POSIX: size=1 writes NUL and returns buf (empty string, no read).
        let mut buf = [0xFFu8; 10];
        let ret = fgets(buf.as_mut_ptr(), 1, STDIN_SENTINEL as *mut u8);
        assert_eq!(ret, buf.as_mut_ptr());
        assert_eq!(buf[0], 0, "size=1 must write NUL terminator");
    }

    // -----------------------------------------------------------------------
    // FORTIFY _chk wrappers — clamping / delegation
    // -----------------------------------------------------------------------

    #[test]
    fn test_fgets_chk_negative_count() {
        let mut buf = [0u8; 10];
        let ret = __fgets_chk(buf.as_mut_ptr(), 10, -1, STDIN_SENTINEL as *mut u8);
        assert!(ret.is_null());
    }

    #[test]
    fn test_fgets_chk_clamps_to_object_size() {
        // size (object) = 1 clamps the effective count to 1, so fgets writes
        // only the NUL terminator and returns buf — never touching the read.
        let mut buf = [0xFFu8; 10];
        let ret = __fgets_chk(buf.as_mut_ptr(), 1, 100, STDIN_SENTINEL as *mut u8);
        assert_eq!(ret, buf.as_mut_ptr());
        assert_eq!(buf[0], 0, "clamp to object size 1 must write only NUL");
    }

    #[test]
    fn test_fread_chk_zero_size() {
        let mut buf = [0u8; 10];
        let ret = unsafe { __fread_chk(buf.as_mut_ptr(), 10, 0, 10, STDIN_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fread_chk_clamps_nmemb_to_object() {
        // ptrlen too small for size*nmemb: nmemb clamps to ptrlen/size = 0,
        // so nothing is read regardless of the requested 10 elements.
        let mut buf = [0u8; 2];
        let ret = unsafe { __fread_chk(buf.as_mut_ptr(), 2, 4, 10, STDIN_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fread_chk_null_ptr() {
        let ret =
            unsafe { __fread_chk(core::ptr::null_mut(), 10, 1, 10, STDIN_SENTINEL as *mut u8) };
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // flockfile / ftrylockfile / funlockfile — stubs
    // -----------------------------------------------------------------------

    #[test]
    fn test_flockfile_is_noop() {
        // Should not crash — our locking is a no-op.
        flockfile(STDOUT_SENTINEL as *mut core::ffi::c_void);
    }

    #[test]
    fn test_ftrylockfile_always_succeeds() {
        assert_eq!(ftrylockfile(STDOUT_SENTINEL as *mut core::ffi::c_void), 0);
    }

    #[test]
    fn test_funlockfile_is_noop() {
        funlockfile(STDOUT_SENTINEL as *mut core::ffi::c_void);
    }

    // -----------------------------------------------------------------------
    // File struct size
    // -----------------------------------------------------------------------

    #[test]
    fn test_file_struct_contains_buffer() {
        // FILE must be large enough to hold the BUF_SIZE buffer.
        let size = core::mem::size_of::<File>();
        assert!(
            size >= BUF_SIZE,
            "File struct ({size} bytes) must be at least BUF_SIZE ({BUF_SIZE})"
        );
    }

    // -----------------------------------------------------------------------
    // alloc_file / free_file round trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_alloc_free_file_round_trip() {
        // Reset pool state to avoid interference.
        // SAFETY: Single-threaded test.
        unsafe {
            let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
            let mut i: usize = 0;
            while i < MAX_OPEN_FILES {
                (*pool.add(i)).in_use = false;
                i += 1;
            }
        }

        let f = alloc_file(42, BUF_MODE_FULL);
        assert!(!f.is_null(), "alloc_file should succeed with empty pool");
        let fd = unsafe { (*f).fd };
        assert_eq!(fd, 42);
        let mode = unsafe { (*f).buf_mode };
        assert_eq!(mode, BUF_MODE_FULL);

        free_file(f);

        // After freeing, should be able to allocate again.
        let f2 = alloc_file(99, BUF_MODE_LINE);
        assert!(!f2.is_null());
        let fd2 = unsafe { (*f2).fd };
        assert_eq!(fd2, 99);
        free_file(f2);
    }

    #[test]
    fn test_alloc_file_pool_exhaustion() {
        // Reset pool.
        unsafe {
            let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
            let mut i: usize = 0;
            while i < MAX_OPEN_FILES {
                (*pool.add(i)).in_use = false;
                i += 1;
            }
        }

        // Allocate all slots.
        let mut ptrs = [core::ptr::null_mut::<File>(); MAX_OPEN_FILES];
        for i in 0..MAX_OPEN_FILES {
            ptrs[i] = alloc_file(i as i32 + 10, BUF_MODE_FULL);
            assert!(!ptrs[i].is_null(), "slot {i} should be available");
        }

        // Next allocation should fail.
        let overflow = alloc_file(999, BUF_MODE_FULL);
        assert!(overflow.is_null(), "pool should be exhausted");

        // Free all.
        for p in &ptrs {
            free_file(*p);
        }
    }

    // -----------------------------------------------------------------------
    // File::new initializer
    // -----------------------------------------------------------------------

    #[test]
    fn test_file_new_various_modes() {
        for mode in [BUF_MODE_FULL, BUF_MODE_LINE, BUF_MODE_NONE] {
            let f = File::new(7, mode);
            assert_eq!(f.fd, 7);
            assert_eq!(f.buf_mode, mode);
            assert_eq!(f.buf_pos, 0);
            assert_eq!(f.buf_len, 0);
            assert_eq!(f.buf_dir, BUF_DIR_IDLE);
            assert_eq!(f.flags, 0);
            assert_eq!(f.ungetc_byte, -1);
        }
    }

    // -----------------------------------------------------------------------
    // remove / stdio_rename — null pointer handling
    // -----------------------------------------------------------------------

    #[test]
    fn test_remove_null() {
        // remove(null) should return error (delegates to unlink).
        let ret = remove(core::ptr::null());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_stdio_rename_null_old() {
        let ret = stdio_rename(core::ptr::null(), b"/tmp/new\0".as_ptr());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_stdio_rename_null_new() {
        let ret = stdio_rename(b"/tmp/old\0".as_ptr(), core::ptr::null());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // setbuf — null sets unbuffered, non-null sets fully buffered
    // -----------------------------------------------------------------------

    #[test]
    fn test_setbuf_null_makes_unbuffered() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };
        setbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut());
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_NONE);
        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    #[test]
    fn test_setbuf_nonnull_makes_fully_buffered() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };
        let mut dummy = [0u8; 1];
        setbuf(STDERR_SENTINEL as *mut u8, dummy.as_mut_ptr());
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_FULL);
        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    // -----------------------------------------------------------------------
    // setlinebuf — sets line-buffered mode
    // -----------------------------------------------------------------------

    #[test]
    fn test_setlinebuf_makes_line_buffered() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };
        setlinebuf(STDERR_SENTINEL as *mut u8);
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_LINE);
        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    // -----------------------------------------------------------------------
    // setbuffer — null sets unbuffered, non-null sets fully buffered
    // -----------------------------------------------------------------------

    #[test]
    fn test_setbuffer_null_makes_unbuffered() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };
        setbuffer(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 0);
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_NONE);
        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    #[test]
    fn test_setbuffer_nonnull_makes_fully_buffered() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };
        let mut dummy = [0u8; 1];
        setbuffer(STDERR_SENTINEL as *mut u8, dummy.as_mut_ptr(), 4096);
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_FULL);
        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    // -----------------------------------------------------------------------
    // setvbuf — mode transitions and buffer state
    // -----------------------------------------------------------------------

    #[test]
    fn test_setvbuf_full_to_line() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };

        // Set to fully buffered first.
        setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 0, 0);
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_FULL);

        // Switch to line-buffered.
        setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 1, 0);
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_LINE);

        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    #[test]
    fn test_setvbuf_line_to_none() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_mode = unsafe { (*file).buf_mode };

        setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 1, 0);
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_LINE);

        setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 2, 0);
        assert_eq!(unsafe { (*file).buf_mode }, BUF_MODE_NONE);

        unsafe {
            (*file).buf_mode = old_mode;
        }
    }

    #[test]
    fn test_setvbuf_clears_read_buffer_on_mode_change() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_dir = unsafe { (*file).buf_dir };
        let old_pos = unsafe { (*file).buf_pos };
        let old_len = unsafe { (*file).buf_len };
        let old_mode = unsafe { (*file).buf_mode };

        // Simulate buffered read state.
        unsafe {
            (*file).buf_dir = BUF_DIR_READ;
            (*file).buf_pos = 10;
            (*file).buf_len = 50;
        }

        setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 2, 0);

        // Buffer state should be cleared.
        assert_eq!(unsafe { (*file).buf_pos }, 0);
        assert_eq!(unsafe { (*file).buf_len }, 0);
        assert_eq!(unsafe { (*file).buf_dir }, BUF_DIR_IDLE);

        // Restore.
        unsafe {
            (*file).buf_dir = old_dir;
            (*file).buf_pos = old_pos;
            (*file).buf_len = old_len;
            (*file).buf_mode = old_mode;
        }
    }

    #[test]
    fn test_setvbuf_returns_zero_on_success() {
        for mode in [0, 1, 2] {
            let ret = setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), mode, 0);
            assert_eq!(ret, 0, "setvbuf(mode={mode}) should return 0");
        }
        // Restore stderr to unbuffered.
        setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 2, 0);
    }

    #[test]
    fn test_setvbuf_returns_neg1_on_invalid() {
        let ret = setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), 3, 0);
        assert_eq!(ret, -1);

        let ret = setvbuf(STDERR_SENTINEL as *mut u8, core::ptr::null_mut(), -1, 0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // putc — alias for fputc
    // -----------------------------------------------------------------------

    #[test]
    fn test_putc_is_fputc_alias() {
        // Both should process the same byte value.
        // We can't test actual I/O, but verify the function exists and
        // is callable without crash for a valid sentinel.
        // putc writes to the stream buffer; in test mode the underlying
        // write syscall returns 0 (writes go to a non-existent fd).
        let ret = putc(b'X' as i32, STDERR_SENTINEL as *mut u8);
        // stderr is unbuffered so it calls write immediately.
        // In test mode write returns 0 (no actual kernel).
        // fputc returns EOF on write error or the byte value on success.
        // Either result is acceptable — we're testing it doesn't crash.
        let _ = ret;
    }

    // -----------------------------------------------------------------------
    // fopen — invalid mode string
    // -----------------------------------------------------------------------

    #[test]
    fn test_fopen_invalid_mode() {
        let ret = unsafe { fopen(b"/tmp/x\0".as_ptr(), b"x\0".as_ptr()) };
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // freopen — null mode
    // -----------------------------------------------------------------------

    #[test]
    fn test_freopen_null_mode() {
        let ret = unsafe {
            freopen(
                b"/tmp/x\0".as_ptr(),
                core::ptr::null(),
                STDOUT_SENTINEL as *mut u8,
            )
        };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // fflush(NULL) — flush all streams (idle buffers succeed)
    // -----------------------------------------------------------------------

    #[test]
    fn test_fflush_null_flushes_all_idle() {
        // Ensure all streams are idle (no pending writes).
        // fflush(NULL) should return 0 when all buffers are idle.
        // Reset FILE_POOL so no in-use files interfere.
        unsafe {
            let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
            let mut i: usize = 0;
            while i < MAX_OPEN_FILES {
                (*pool.add(i)).in_use = false;
                i += 1;
            }
        }

        // Reset stdout and stderr write buffers to idle.
        let f_out = stream_to_file(STDOUT_SENTINEL as *mut u8);
        let f_err = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_out_dir = unsafe { (*f_out).buf_dir };
        let old_out_pos = unsafe { (*f_out).buf_pos };
        let old_err_dir = unsafe { (*f_err).buf_dir };
        let old_err_pos = unsafe { (*f_err).buf_pos };

        unsafe {
            (*f_out).buf_dir = BUF_DIR_IDLE;
            (*f_out).buf_pos = 0;
            (*f_err).buf_dir = BUF_DIR_IDLE;
            (*f_err).buf_pos = 0;
        }

        let ret = fflush(core::ptr::null_mut());
        assert_eq!(ret, 0, "fflush(NULL) should succeed on idle buffers");

        // Restore.
        unsafe {
            (*f_out).buf_dir = old_out_dir;
            (*f_out).buf_pos = old_out_pos;
            (*f_err).buf_dir = old_err_dir;
            (*f_err).buf_pos = old_err_pos;
        }
    }

    // -----------------------------------------------------------------------
    // fflush on a specific stream with idle buffer
    // -----------------------------------------------------------------------

    #[test]
    fn test_fflush_idle_stream_returns_zero() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_dir = unsafe { (*file).buf_dir };
        let old_pos = unsafe { (*file).buf_pos };

        unsafe {
            (*file).buf_dir = BUF_DIR_IDLE;
            (*file).buf_pos = 0;
        }

        let ret = fflush(STDERR_SENTINEL as *mut u8);
        assert_eq!(ret, 0);

        unsafe {
            (*file).buf_dir = old_dir;
            (*file).buf_pos = old_pos;
        }
    }

    // -----------------------------------------------------------------------
    // rewind — clears error flag
    // -----------------------------------------------------------------------

    #[test]
    fn test_rewind_clears_error() {
        let file = stream_to_file(STDERR_SENTINEL as *mut u8);
        let old_flags = unsafe { (*file).flags };

        // Set error flag.
        unsafe {
            (*file).flags |= FLAG_ERR;
        }

        rewind(STDERR_SENTINEL as *mut u8);
        assert_eq!(
            unsafe { (*file).flags } & FLAG_ERR,
            0,
            "rewind must clear error"
        );

        unsafe {
            (*file).flags = old_flags;
        }
    }

    // -----------------------------------------------------------------------
    // fseeko / ftello / fseeko64 / ftello64 — alias correctness
    // -----------------------------------------------------------------------

    #[test]
    fn test_fseeko_is_fseek_alias() {
        // Both should produce the same result for the same arguments.
        // On LP64, fseeko == fseek — verify the function exists.
        let _ret = fseeko(STDERR_SENTINEL as *mut u8, 0, SEEK_SET);
        // Can't verify exact result (depends on lseek syscall in test mode)
        // but it should not crash.
    }

    #[test]
    fn test_ftello_is_ftell_alias() {
        let _ret = ftello(STDERR_SENTINEL as *mut u8);
    }

    #[test]
    fn test_fseeko64_is_fseek_alias() {
        let _ret = fseeko64(STDERR_SENTINEL as *mut u8, 0, SEEK_SET);
    }

    #[test]
    fn test_ftello64_is_ftell_alias() {
        let _ret = ftello64(STDERR_SENTINEL as *mut u8);
    }

    // -----------------------------------------------------------------------
    // fgetpos64 / fsetpos64 — LP64 aliases
    // -----------------------------------------------------------------------

    #[test]
    fn test_fgetpos64_null_pos() {
        let ret = fgetpos64(STDOUT_SENTINEL as *mut u8, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fsetpos64_null_pos() {
        let ret = fsetpos64(STDOUT_SENTINEL as *mut u8, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // fopen64 — LP64 alias for fopen
    // -----------------------------------------------------------------------

    #[test]
    fn test_fopen64_null_path() {
        let ret = unsafe { fopen64(core::ptr::null(), b"r\0".as_ptr()) };
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fopen64_null_mode() {
        let ret = unsafe { fopen64(b"/tmp/x\0".as_ptr(), core::ptr::null()) };
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // Unlocked variants — aliases for locked versions
    // -----------------------------------------------------------------------

    #[test]
    fn test_getc_unlocked_exists() {
        // getc_unlocked(stream) is an alias for fgetc(stream).
        // Verify it's callable without crashing.  The return value
        // depends on stdin state (may have a pushed-back byte from
        // a prior test), so we only assert it doesn't panic.
        let _ret = getc_unlocked(STDIN_SENTINEL as *mut u8);
    }

    #[test]
    fn test_getchar_unlocked_exists() {
        // Alias for fgetc(stdin).  Just verify it doesn't crash.
        let _ret = getchar_unlocked();
    }

    #[test]
    fn test_putc_unlocked_exists() {
        let _ret = putc_unlocked(b'X' as i32, STDERR_SENTINEL as *mut u8);
    }

    #[test]
    fn test_putchar_unlocked_exists() {
        let _ret = putchar_unlocked(b'X' as i32);
    }

    // -----------------------------------------------------------------------
    // File pool — free non-pool pointer is a no-op
    // -----------------------------------------------------------------------

    #[test]
    fn test_free_file_nonpool_is_noop() {
        // Freeing a pointer not in the pool should do nothing (no crash).
        let mut fake = File::new(99, BUF_MODE_FULL);
        free_file(&raw mut fake);
        // If we get here, it didn't crash — success.
    }

    // -----------------------------------------------------------------------
    // mode_to_flags — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_mode_to_flags_empty_mode() {
        // A NUL byte at position 0 means the mode char is 0, which
        // is not r/w/a → invalid.
        assert_eq!(mode_to_flags(b"\0".as_ptr()), -1);
    }

    // -----------------------------------------------------------------------
    // ungetc — double pushback
    // -----------------------------------------------------------------------

    #[test]
    fn test_ungetc_overwrites_previous() {
        let file = stream_to_file(STDIN_SENTINEL as *mut u8);
        let old_byte = unsafe { (*file).ungetc_byte };
        let old_flags = unsafe { (*file).flags };

        ungetc(b'A' as i32, STDIN_SENTINEL as *mut u8);
        assert_eq!(unsafe { (*file).ungetc_byte }, b'A' as i16);

        // Second pushback overwrites the first.
        ungetc(b'B' as i32, STDIN_SENTINEL as *mut u8);
        assert_eq!(unsafe { (*file).ungetc_byte }, b'B' as i16);

        unsafe {
            (*file).ungetc_byte = old_byte;
            (*file).flags = old_flags;
        }
    }

    // -----------------------------------------------------------------------
    // fdopen — pool allocation (non-standard fds)
    // -----------------------------------------------------------------------

    #[test]
    fn test_fdopen_allocates_from_pool() {
        // Reset pool.
        unsafe {
            let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
            let mut i: usize = 0;
            while i < MAX_OPEN_FILES {
                (*pool.add(i)).in_use = false;
                i += 1;
            }
        }

        let stream = fdopen(42, b"r\0".as_ptr());
        assert!(!stream.is_null(), "fdopen(42) should allocate from pool");

        // Check it's a valid File pointer.
        let file = stream_to_file(stream);
        assert_eq!(unsafe { (*file).fd }, 42);

        // Clean up — return the slot.
        free_file(file);
    }

    // -----------------------------------------------------------------------
    // fclose — closing streams
    // -----------------------------------------------------------------------

    #[test]
    fn test_fclose_stdin_flushes_only() {
        // fclose on stdin should flush but NOT actually close fd 0.
        let ret = fclose(STDIN_SENTINEL as *mut u8);
        // Standard streams are not freed; fclose returns 0 on success.
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fclose_stdout_flushes_only() {
        let ret = fclose(STDOUT_SENTINEL as *mut u8);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fclose_stderr_flushes_only() {
        let ret = fclose(STDERR_SENTINEL as *mut u8);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_fclose_pool_file() {
        // Allocate a pool file, then close it.
        unsafe {
            let pool = core::ptr::addr_of_mut!(FILE_POOL).cast::<FileSlot>();
            let mut i: usize = 0;
            while i < MAX_OPEN_FILES {
                (*pool.add(i)).in_use = false;
                i += 1;
            }
        }
        let stream = fdopen(99, b"r\0".as_ptr());
        assert!(!stream.is_null());
        // fclose will call close(99) which may fail on host, but the
        // pool slot should be freed regardless.
        let _ret = fclose(stream);
        // The slot should be freed — verify we can re-allocate.
        let stream2 = fdopen(100, b"r\0".as_ptr());
        assert!(!stream2.is_null());
        free_file(stream_to_file(stream2));
    }

    // -----------------------------------------------------------------------
    // fgetc — reading characters
    // -----------------------------------------------------------------------

    #[test]
    fn test_fgetc_stdin_no_crash() {
        // fgetc reads from a stream. On the test host the stdin fd may
        // return EOF immediately (not a tty), but must not crash.
        let _ret = fgetc(STDIN_SENTINEL as *mut u8);
        // We can't assert the value — it depends on host stdin state.
    }

    #[test]
    fn test_fgetc_returns_ungetc_byte() {
        // Push back a byte, then read it with fgetc.
        let file = stream_to_file(STDIN_SENTINEL as *mut u8);
        let old_byte = unsafe { (*file).ungetc_byte };
        let old_flags = unsafe { (*file).flags };

        unsafe {
            (*file).ungetc_byte = b'Z' as i16;
        }
        // Clear EOF so fgetc checks the pushback buffer.
        unsafe {
            (*file).flags &= !FLAG_EOF;
        }

        let c = fgetc(STDIN_SENTINEL as *mut u8);
        assert_eq!(c, b'Z' as i32, "fgetc should return the pushed-back byte");

        // Restore.
        unsafe {
            (*file).ungetc_byte = old_byte;
            (*file).flags = old_flags;
        }
    }

    #[test]
    fn test_getchar_no_crash() {
        let _ret = getchar();
    }

    #[test]
    fn test_getc_no_crash() {
        let _ret = getc(STDIN_SENTINEL as *mut u8);
    }

    // -----------------------------------------------------------------------
    // freopen64 — LP64 alias
    // -----------------------------------------------------------------------

    #[test]
    fn test_freopen64_null_mode() {
        let ret = unsafe {
            freopen64(
                b"/tmp/x\0".as_ptr(),
                core::ptr::null(),
                STDOUT_SENTINEL as *mut u8,
            )
        };
        assert!(ret.is_null(), "freopen64 with null mode should fail");
    }

    #[test]
    fn test_freopen64_null_path_returns_stream() {
        // With null path and valid mode, freopen returns the same stream
        // (mode-change-only path, which is "not supported" → returns stream).
        let ret = unsafe {
            freopen64(
                core::ptr::null(),
                b"r\0".as_ptr(),
                STDOUT_SENTINEL as *mut u8,
            )
        };
        assert_eq!(ret as usize, STDOUT_SENTINEL);
    }

    // -----------------------------------------------------------------------
    // getline / getdelim — line reading
    // -----------------------------------------------------------------------

    #[test]
    fn test_getline_null_lineptr() {
        let mut n: usize = 0;
        let ret = unsafe { getline(core::ptr::null_mut(), &raw mut n, STDIN_SENTINEL as *mut u8) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_getline_null_n() {
        let mut ptr: *mut u8 = core::ptr::null_mut();
        let ret = unsafe {
            getline(
                &raw mut ptr,
                core::ptr::null_mut(),
                STDIN_SENTINEL as *mut u8,
            )
        };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_getdelim_custom_delimiter_null_inputs() {
        // getdelim with ';' delimiter and null lineptr → EINVAL
        let mut n: usize = 0;
        crate::errno::set_errno(0);
        let ret = unsafe {
            getdelim(
                core::ptr::null_mut(),
                &raw mut n,
                b';' as i32,
                STDIN_SENTINEL as *mut u8,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getdelim_initial_null_buf_allocates() {
        // If lineptr starts as null and n starts as 0, getdelim should
        // try to malloc(128).  On test host, stdin will likely EOF
        // immediately, so we get -1 but the allocation still happens.
        let mut ptr: *mut u8 = core::ptr::null_mut();
        let mut n: usize = 0;
        let ret = unsafe {
            getdelim(
                &raw mut ptr,
                &raw mut n,
                b'\n' as i32,
                STDIN_SENTINEL as *mut u8,
            )
        };
        // Either -1 (EOF) or some positive value depending on host stdin.
        // The point: if allocation succeeded, n and ptr were updated.
        if ret == -1 {
            // Allocation may or may not have happened (malloc could fail,
            // or EOF before any data). Either way, no crash.
        }
        // Clean up if a buffer was allocated.
        if !ptr.is_null() {
            unsafe {
                crate::malloc::free(ptr);
            }
        }
    }

    // -----------------------------------------------------------------------
    // perror — error message printing
    // -----------------------------------------------------------------------

    #[test]
    fn test_perror_null_prefix() {
        // perror(NULL) should print just "strerror(errno)\n".
        crate::errno::set_errno(crate::errno::ENOENT);
        unsafe {
            perror(core::ptr::null());
        }
    }

    #[test]
    fn test_perror_empty_prefix() {
        // perror("") should print just "strerror(errno)\n" (empty prefix
        // is treated as no prefix because *s == 0).
        crate::errno::set_errno(crate::errno::EACCES);
        unsafe {
            perror(b"\0".as_ptr());
        }
    }

    #[test]
    fn test_perror_with_prefix() {
        crate::errno::set_errno(crate::errno::EIO);
        unsafe {
            perror(b"test_perror\0".as_ptr());
        }
        // Should print "test_perror: Input/output error\n" to stderr.
    }

    #[test]
    fn test_perror_zero_errno() {
        crate::errno::set_errno(0);
        unsafe {
            perror(b"no error\0".as_ptr());
        }
    }

    #[test]
    fn test_perror_various_errno_values() {
        for e in [
            crate::errno::EPERM,
            crate::errno::ENOMEM,
            crate::errno::EINVAL,
            crate::errno::ENOSYS,
        ] {
            crate::errno::set_errno(e);
            unsafe {
                perror(b"loop\0".as_ptr());
            }
        }
    }
}
