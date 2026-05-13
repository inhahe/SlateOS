//! POSIX errno — per-thread error number.
//!
//! The POSIX errno convention: functions return -1 and set `errno` to
//! indicate which error occurred.  Our native syscalls return negative
//! error codes directly.  This module translates between the two.
//!
//! Since we're `no_std` without threads yet, errno is a simple global.
//! When threading is added, this will become a thread-local via TLS.

use core::sync::atomic::{AtomicI32, Ordering};

// ---------------------------------------------------------------------------
// POSIX errno values
// ---------------------------------------------------------------------------

// These values match Linux x86_64 for maximum compatibility.

pub const EPERM: i32 = 1;          // Operation not permitted
pub const ENOENT: i32 = 2;         // No such file or directory
pub const ESRCH: i32 = 3;          // No such process
pub const EINTR: i32 = 4;          // Interrupted system call
pub const EIO: i32 = 5;            // I/O error
pub const ENXIO: i32 = 6;          // No such device or address
pub const E2BIG: i32 = 7;          // Argument list too long
pub const ENOEXEC: i32 = 8;        // Exec format error
pub const EBADF: i32 = 9;          // Bad file descriptor
pub const ECHILD: i32 = 10;        // No child processes
pub const EAGAIN: i32 = 11;        // Resource temporarily unavailable
pub const ENOMEM: i32 = 12;        // Cannot allocate memory
pub const EACCES: i32 = 13;        // Permission denied
pub const EFAULT: i32 = 14;        // Bad address
pub const EBUSY: i32 = 16;         // Device or resource busy
pub const EEXIST: i32 = 17;        // File exists
pub const EXDEV: i32 = 18;         // Invalid cross-device link
pub const ENODEV: i32 = 19;        // No such device
pub const ENOTDIR: i32 = 20;       // Not a directory
pub const EISDIR: i32 = 21;        // Is a directory
pub const EINVAL: i32 = 22;        // Invalid argument
pub const ENFILE: i32 = 23;        // Too many open files in system
pub const EMFILE: i32 = 24;        // Too many open files
pub const ENOTTY: i32 = 25;        // Inappropriate ioctl for device
pub const EFBIG: i32 = 27;         // File too large
pub const ENOSPC: i32 = 28;        // No space left on device
pub const ESPIPE: i32 = 29;        // Illegal seek
pub const EROFS: i32 = 30;         // Read-only file system
pub const EMLINK: i32 = 31;        // Too many links
pub const EPIPE: i32 = 32;         // Broken pipe
pub const EDOM: i32 = 33;          // Numerical argument out of domain
pub const ERANGE: i32 = 34;        // Numerical result out of range
pub const EDEADLK: i32 = 35;       // Resource deadlock avoided
pub const ENAMETOOLONG: i32 = 36;   // File name too long
pub const ENOLCK: i32 = 37;        // No locks available
pub const ENOSYS: i32 = 38;        // Function not implemented
pub const ENOTEMPTY: i32 = 39;      // Directory not empty
pub const ELOOP: i32 = 40;         // Too many levels of symbolic links
pub const EWOULDBLOCK: i32 = EAGAIN;
pub const ENOMSG: i32 = 42;        // No message of desired type
pub const ENODATA: i32 = 61;       // No data available
pub const ETIME: i32 = 62;         // Timer expired
pub const EOVERFLOW: i32 = 75;     // Value too large for data type
pub const ENOTSOCK: i32 = 88;      // Socket operation on non-socket
pub const EDESTADDRREQ: i32 = 89;  // Destination address required
pub const ENOPROTOOPT: i32 = 92;   // Protocol not available
pub const EPROTONOSUPPORT: i32 = 93; // Protocol not supported
pub const ENOTSUP: i32 = 95;       // Operation not supported
pub const EOPNOTSUPP: i32 = 95;    // Operation not supported on socket (same as ENOTSUP on Linux)
pub const EAFNOSUPPORT: i32 = 97;  // Address family not supported
pub const EADDRINUSE: i32 = 98;    // Address already in use
pub const EADDRNOTAVAIL: i32 = 99; // Cannot assign requested address
pub const ENETUNREACH: i32 = 101;  // Network is unreachable
pub const ECONNRESET: i32 = 104;   // Connection reset by peer
pub const EISCONN: i32 = 106;      // Transport endpoint is already connected
pub const ENOTCONN: i32 = 107;     // Transport endpoint is not connected
pub const ETIMEDOUT: i32 = 110;    // Connection timed out
pub const ESHUTDOWN: i32 = 108;    // Cannot send after transport shutdown
pub const ECONNREFUSED: i32 = 111; // Connection refused
pub const EHOSTDOWN: i32 = 112;    // Host is down
pub const EHOSTUNREACH: i32 = 113; // No route to host
pub const EALREADY: i32 = 114;     // Operation already in progress
pub const EINPROGRESS: i32 = 115;  // Operation now in progress
pub const ECANCELED: i32 = 125;    // Operation canceled
pub const ENOMEDIUM: i32 = 123;    // No medium found
pub const EILSEQ: i32 = 84;        // Invalid or incomplete multibyte/wide character
pub const EOWNERDEAD: i32 = 130;   // Owner died
pub const ENOTRECOVERABLE: i32 = 131; // State not recoverable
pub const ENOLINK: i32 = 67;       // Link has been severed
pub const EPROTO: i32 = 71;        // Protocol error
pub const EMULTIHOP: i32 = 72;     // Multihop attempted
pub const EBADMSG: i32 = 74;       // Bad message
pub const EIDRM: i32 = 43;         // Identifier removed
pub const ENOSR: i32 = 63;         // Out of streams resources
pub const ENOSTR: i32 = 60;        // Device not a stream
pub const ESTALE: i32 = 116;       // Stale file handle
pub const EMSGSIZE: i32 = 90;      // Message too long
pub const EPROTOTYPE: i32 = 91;    // Protocol wrong type for socket
pub const ENETDOWN: i32 = 100;     // Network is down
pub const ENETRESET: i32 = 102;    // Network dropped connection on reset
pub const ECONNABORTED: i32 = 103; // Software caused connection abort
pub const ENOBUFS: i32 = 105;      // No buffer space available
pub const ETXTBSY: i32 = 26;       // Text file busy

// ---------------------------------------------------------------------------
// Per-thread errno storage
// ---------------------------------------------------------------------------

// TODO: Replace with proper TLS when threading is supported.
// For now, a single atomic is sufficient for single-threaded userspace.
static ERRNO: AtomicI32 = AtomicI32::new(0);

/// Set errno.
#[inline]
pub fn set_errno(val: i32) {
    ERRNO.store(val, Ordering::Relaxed);
}

/// Get errno.
#[inline]
#[must_use]
pub fn get_errno() -> i32 {
    ERRNO.load(Ordering::Relaxed)
}

/// C-compatible errno access.
///
/// Returns a pointer to the errno variable.  C programs access errno
/// via `*__errno_location()`.  This is the glibc/musl convention.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __errno_location() -> *mut i32 {
    // SAFETY: AtomicI32 has the same layout as i32.
    // The pointer is valid for the lifetime of the program.
    // Using &raw const to avoid borrow_as_ptr lint (Rust 2024 idiom).
    (&raw const ERRNO).cast_mut().cast::<i32>()
}

// ---------------------------------------------------------------------------
// Native error code → POSIX errno translation
// ---------------------------------------------------------------------------

/// Our kernel error codes (from kernel/src/error.rs `KernelError` enum).
///
/// These are the negative values returned by native syscalls.
/// MUST stay in sync with kernel/src/error.rs — any mismatch causes
/// wrong errno values throughout the entire POSIX layer.
pub(crate) mod native {
    // --- General (0-99 range: -1 to -6) ---
    pub const INTERNAL_ERROR: i64 = -1;
    pub const NOT_SUPPORTED: i64 = -2;
    pub const INVALID_ARGUMENT: i64 = -3;
    pub const WOULD_BLOCK: i64 = -4;
    pub const CANCELLED: i64 = -5;
    pub const TIMED_OUT: i64 = -6;

    // --- Memory (100 range: -100 to -103) ---
    pub const OUT_OF_MEMORY: i64 = -100;
    pub const INVALID_ADDRESS: i64 = -101;
    // PageFault = -102 (not typically returned to userspace)
    // BadAlignment = -103

    // --- Process (200 range: -200 to -202) ---
    pub const NO_SUCH_PROCESS: i64 = -200;
    // InvalidExecutable = -201
    // ProcessExited = -202

    // --- IPC (300 range: -300 to -304) ---
    pub const CHANNEL_CLOSED: i64 = -300;
    pub const CHANNEL_FULL: i64 = -301;
    // MessageTooLarge = -302
    // Overflow = -303
    pub const RESOURCE_EXHAUSTED: i64 = -304;

    // --- Capability (400 range: -400 to -401) ---
    pub const PERMISSION_DENIED: i64 = -400;
    // InvalidCapability = -401

    // --- Filesystem (500 range: -500 to -511) ---
    pub const NOT_FOUND: i64 = -500;
    pub const ALREADY_EXISTS: i64 = -501;
    pub const NOT_A_DIRECTORY: i64 = -502;
    pub const IS_A_DIRECTORY: i64 = -503;
    pub const NO_SPACE: i64 = -504;
    pub const BAD_HANDLE: i64 = -505;
    pub const TOO_MANY_LINKS: i64 = -506;
    pub const DIRECTORY_NOT_EMPTY: i64 = -507;
    // CorruptedData = -508
    pub const READ_ONLY_FS: i64 = -509;
    pub const TOO_MANY_OPEN_FILES: i64 = -510;
    pub const FILE_TOO_LARGE: i64 = -511;

    // --- Device / I/O (600 range: -600 to -602) ---
    pub const IO_ERROR: i64 = -600;
    pub const NO_SUCH_DEVICE: i64 = -601;
    pub const RESOURCE_BUSY: i64 = -602;
}

/// Translate a native syscall return value to POSIX convention.
///
/// - If `ret >= 0`, returns `ret` (success).
/// - If `ret < 0`, sets `errno` and returns `-1`.
#[inline]
#[must_use]
pub fn translate(ret: i64) -> i64 {
    if ret >= 0 {
        return ret;
    }

    #[allow(clippy::match_same_arms)] // Kept separate for readability: each
    // native error code documents its semantic mapping even when the POSIX
    // target is the same (e.g. INTERNAL_ERROR and IO_ERROR both → EIO).
    let err = match ret {
        // General errors
        native::INTERNAL_ERROR => EIO,
        native::NOT_SUPPORTED => ENOTSUP,
        native::INVALID_ARGUMENT => EINVAL,
        native::WOULD_BLOCK | native::CHANNEL_FULL => EAGAIN,
        native::CANCELLED => ECANCELED,
        native::TIMED_OUT => ETIMEDOUT,

        // Memory errors
        native::OUT_OF_MEMORY | native::RESOURCE_EXHAUSTED => ENOMEM,
        native::INVALID_ADDRESS => EFAULT,

        // Process errors
        native::NO_SUCH_PROCESS => ESRCH,

        // IPC errors
        native::CHANNEL_CLOSED => ECONNRESET,

        // Capability / permission errors
        native::PERMISSION_DENIED => EACCES,

        // Filesystem errors
        native::NOT_FOUND => ENOENT,
        native::ALREADY_EXISTS => EEXIST,
        native::NOT_A_DIRECTORY => ENOTDIR,
        native::IS_A_DIRECTORY => EISDIR,
        native::NO_SPACE => ENOSPC,
        native::BAD_HANDLE => EBADF,
        native::TOO_MANY_LINKS => ELOOP,
        native::DIRECTORY_NOT_EMPTY => ENOTEMPTY,
        native::READ_ONLY_FS => EROFS,
        native::TOO_MANY_OPEN_FILES => EMFILE,
        native::FILE_TOO_LARGE => EFBIG,

        // Device / I/O errors
        native::IO_ERROR => EIO,
        native::NO_SUCH_DEVICE => ENODEV,
        native::RESOURCE_BUSY => EBUSY,

        _ => EIO, // Unknown error → generic I/O error.
    };

    set_errno(err);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_get_errno() {
        set_errno(0);
        assert_eq!(get_errno(), 0);

        set_errno(ENOENT);
        assert_eq!(get_errno(), ENOENT);

        set_errno(EINVAL);
        assert_eq!(get_errno(), EINVAL);
    }

    #[test]
    fn test_translate_success() {
        set_errno(0);
        let result = translate(42);
        assert_eq!(result, 42);
        assert_eq!(get_errno(), 0); // errno unchanged on success.
    }

    #[test]
    fn test_translate_zero() {
        set_errno(99);
        let result = translate(0);
        assert_eq!(result, 0);
        assert_eq!(get_errno(), 99); // errno unchanged on success.
    }

    #[test]
    fn test_translate_not_found() {
        let result = translate(native::NOT_FOUND);
        assert_eq!(result, -1);
        assert_eq!(get_errno(), ENOENT);
    }

    #[test]
    fn test_translate_already_exists() {
        let result = translate(native::ALREADY_EXISTS);
        assert_eq!(result, -1);
        assert_eq!(get_errno(), EEXIST);
    }

    #[test]
    fn test_translate_invalid_argument() {
        let result = translate(native::INVALID_ARGUMENT);
        assert_eq!(result, -1);
        assert_eq!(get_errno(), EINVAL);
    }

    #[test]
    fn test_translate_out_of_memory() {
        let result = translate(native::OUT_OF_MEMORY);
        assert_eq!(result, -1);
        assert_eq!(get_errno(), ENOMEM);
    }

    #[test]
    fn test_translate_would_block() {
        let result = translate(native::WOULD_BLOCK);
        assert_eq!(result, -1);
        assert_eq!(get_errno(), EAGAIN);
    }

    #[test]
    fn test_translate_unknown_error() {
        let result = translate(-9999);
        assert_eq!(result, -1);
        assert_eq!(get_errno(), EIO);
    }

    #[test]
    fn test_errno_constants_match_linux() {
        // Verify key errno values match Linux x86_64 for compatibility.
        assert_eq!(EPERM, 1);
        assert_eq!(ENOENT, 2);
        assert_eq!(EINTR, 4);
        assert_eq!(EIO, 5);
        assert_eq!(EBADF, 9);
        assert_eq!(ENOMEM, 12);
        assert_eq!(EACCES, 13);
        assert_eq!(EEXIST, 17);
        assert_eq!(EINVAL, 22);
        assert_eq!(ENOSYS, 38);
        assert_eq!(ENOTSOCK, 88);
        assert_eq!(ECONNREFUSED, 111);
    }

    #[test]
    fn test_ewouldblock_equals_eagain() {
        assert_eq!(EWOULDBLOCK, EAGAIN);
    }

    #[test]
    fn test_eopnotsupp_equals_enotsup() {
        assert_eq!(EOPNOTSUPP, ENOTSUP);
    }

    #[test]
    fn test_errno_location() {
        set_errno(42);
        let ptr = __errno_location();
        assert!(!ptr.is_null());
        // SAFETY: ptr is valid and points to the errno atomic.
        assert_eq!(unsafe { *ptr }, 42);
    }
}
