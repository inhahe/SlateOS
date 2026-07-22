//! POSIX errno — per-thread error number.
//!
//! The POSIX errno convention: functions return -1 and set `errno` to
//! indicate which error occurred.  Our native syscalls return negative
//! error codes directly.  This module translates between the two.
//!
//! Since we're `no_std` without threads yet, errno is a simple global.
//! When threading is added, this will become a thread-local via TLS.

#[cfg(not(test))]
use core::sync::atomic::{AtomicI32, Ordering};

// ---------------------------------------------------------------------------
// POSIX errno values
// ---------------------------------------------------------------------------

// These values match Linux x86_64 for maximum compatibility.

pub const EPERM: i32 = 1; // Operation not permitted
pub const ENOENT: i32 = 2; // No such file or directory
pub const ESRCH: i32 = 3; // No such process
pub const EINTR: i32 = 4; // Interrupted system call
pub const EIO: i32 = 5; // I/O error
pub const ENXIO: i32 = 6; // No such device or address
pub const E2BIG: i32 = 7; // Argument list too long
pub const ENOEXEC: i32 = 8; // Exec format error
pub const EBADF: i32 = 9; // Bad file descriptor
pub const ECHILD: i32 = 10; // No child processes
pub const EAGAIN: i32 = 11; // Resource temporarily unavailable
pub const ENOMEM: i32 = 12; // Cannot allocate memory
pub const EACCES: i32 = 13; // Permission denied
pub const EFAULT: i32 = 14; // Bad address
pub const ENOTBLK: i32 = 15; // Block device required
pub const EBUSY: i32 = 16; // Device or resource busy
pub const EEXIST: i32 = 17; // File exists
pub const EXDEV: i32 = 18; // Invalid cross-device link
pub const ENODEV: i32 = 19; // No such device
pub const ENOTDIR: i32 = 20; // Not a directory
pub const EISDIR: i32 = 21; // Is a directory
pub const EINVAL: i32 = 22; // Invalid argument
pub const ENFILE: i32 = 23; // Too many open files in system
pub const EMFILE: i32 = 24; // Too many open files
pub const ENOTTY: i32 = 25; // Inappropriate ioctl for device
pub const EFBIG: i32 = 27; // File too large
pub const ENOSPC: i32 = 28; // No space left on device
pub const ESPIPE: i32 = 29; // Illegal seek
pub const EROFS: i32 = 30; // Read-only file system
pub const EMLINK: i32 = 31; // Too many links
pub const EPIPE: i32 = 32; // Broken pipe
pub const EDOM: i32 = 33; // Numerical argument out of domain
pub const ERANGE: i32 = 34; // Numerical result out of range
pub const EDEADLK: i32 = 35; // Resource deadlock avoided
pub const ENAMETOOLONG: i32 = 36; // File name too long
pub const ENOLCK: i32 = 37; // No locks available
pub const ENOSYS: i32 = 38; // Function not implemented
pub const ENOTEMPTY: i32 = 39; // Directory not empty
pub const ELOOP: i32 = 40; // Too many levels of symbolic links
pub const EWOULDBLOCK: i32 = EAGAIN;
pub const ENOMSG: i32 = 42; // No message of desired type
pub const ECHRNG: i32 = 44; // Channel number out of range
pub const EL2NSYNC: i32 = 45; // Level 2 not synchronized
pub const EL3HLT: i32 = 46; // Level 3 halted
pub const EL3RST: i32 = 47; // Level 3 reset
pub const ELNRNG: i32 = 48; // Link number out of range
pub const EUNATCH: i32 = 49; // Protocol driver not attached
pub const ENOCSI: i32 = 50; // No CSI structure available
pub const EL2HLT: i32 = 51; // Level 2 halted
pub const EBADE: i32 = 52; // Invalid exchange
pub const EBADR: i32 = 53; // Invalid request descriptor
pub const EXFULL: i32 = 54; // Exchange full
pub const ENOANO: i32 = 55; // No anode
pub const EBADRQC: i32 = 56; // Invalid request code
pub const EBADSLT: i32 = 57; // Invalid slot
pub const EBFONT: i32 = 59; // Bad font file format
pub const ENODATA: i32 = 61; // No data available
pub const ETIME: i32 = 62; // Timer expired
pub const EOVERFLOW: i32 = 75; // Value too large for data type
pub const ENOTUNIQ: i32 = 76; // Name not unique on network
pub const EBADFD: i32 = 77; // File descriptor in bad state
pub const EREMCHG: i32 = 78; // Remote address changed
pub const ELIBACC: i32 = 79; // Cannot access a shared library
pub const ELIBBAD: i32 = 80; // Accessing a corrupt shared library
pub const ELIBSCN: i32 = 81; // .lib section in a.out corrupted
pub const ELIBMAX: i32 = 82; // Too many shared libraries
pub const ELIBEXEC: i32 = 83; // Cannot exec a shared library directly
pub const ENOTSOCK: i32 = 88; // Socket operation on non-socket
pub const EDESTADDRREQ: i32 = 89; // Destination address required
pub const ENOPROTOOPT: i32 = 92; // Protocol not available
pub const EPROTONOSUPPORT: i32 = 93; // Protocol not supported
pub const ESOCKTNOSUPPORT: i32 = 94; // Socket type not supported
pub const ENOTSUP: i32 = 95; // Operation not supported
pub const EOPNOTSUPP: i32 = 95; // Operation not supported on socket (same as ENOTSUP on Linux)
pub const EPFNOSUPPORT: i32 = 96; // Protocol family not supported
pub const EAFNOSUPPORT: i32 = 97; // Address family not supported
pub const EADDRINUSE: i32 = 98; // Address already in use
pub const EADDRNOTAVAIL: i32 = 99; // Cannot assign requested address
pub const ENETUNREACH: i32 = 101; // Network is unreachable
pub const ECONNRESET: i32 = 104; // Connection reset by peer
pub const EISCONN: i32 = 106; // Transport endpoint is already connected
pub const ENOTCONN: i32 = 107; // Transport endpoint is not connected
pub const ETOOMANYREFS: i32 = 109; // Too many references: cannot splice
pub const ETIMEDOUT: i32 = 110; // Connection timed out
pub const ESHUTDOWN: i32 = 108; // Cannot send after transport shutdown
pub const ECONNREFUSED: i32 = 111; // Connection refused
pub const EHOSTDOWN: i32 = 112; // Host is down
pub const EHOSTUNREACH: i32 = 113; // No route to host
pub const EALREADY: i32 = 114; // Operation already in progress
pub const EINPROGRESS: i32 = 115; // Operation now in progress
pub const ECANCELED: i32 = 125; // Operation canceled
pub const ENOKEY: i32 = 126; // Required key not available
pub const EKEYEXPIRED: i32 = 127; // Key has expired
pub const EKEYREVOKED: i32 = 128; // Key has been revoked
pub const EKEYREJECTED: i32 = 129; // Key was rejected by service
pub const EDEADLOCK: i32 = EDEADLK; // Alias for EDEADLK
pub const ENOMEDIUM: i32 = 123; // No medium found
pub const EMEDIUMTYPE: i32 = 124; // Wrong medium type
pub const EILSEQ: i32 = 84; // Invalid or incomplete multibyte/wide character
pub const ERESTART: i32 = 85; // Interrupted system call should be restarted
pub const ESTRPIPE: i32 = 86; // Streams pipe error
pub const EUSERS: i32 = 87; // Too many users
pub const EOWNERDEAD: i32 = 130; // Owner died
pub const ENOTRECOVERABLE: i32 = 131; // State not recoverable
pub const ENONET: i32 = 64; // Machine is not on the network
pub const ENOPKG: i32 = 65; // Package not installed
pub const EREMOTE: i32 = 66; // Object is remote
pub const ENOLINK: i32 = 67; // Link has been severed
pub const EADV: i32 = 68; // Advertise error
pub const ESRMNT: i32 = 69; // Srmount error
pub const ECOMM: i32 = 70; // Communication error on send
pub const EPROTO: i32 = 71; // Protocol error
pub const EMULTIHOP: i32 = 72; // Multihop attempted
pub const EDOTDOT: i32 = 73; // RFS specific error
pub const EBADMSG: i32 = 74; // Bad message
pub const EIDRM: i32 = 43; // Identifier removed
pub const ENOSR: i32 = 63; // Out of streams resources
pub const ENOSTR: i32 = 60; // Device not a stream
pub const ESTALE: i32 = 116; // Stale file handle
pub const EUCLEAN: i32 = 117; // Structure needs cleaning
pub const ENOTNAM: i32 = 118; // Not a XENIX named type file
pub const ENAVAIL: i32 = 119; // No XENIX semaphores available
pub const EISNAM: i32 = 120; // Is a named type file
pub const EREMOTEIO: i32 = 121; // Remote I/O error
pub const EDQUOT: i32 = 122; // Disk quota exceeded
pub const EMSGSIZE: i32 = 90; // Message too long
pub const EPROTOTYPE: i32 = 91; // Protocol wrong type for socket
pub const ENETDOWN: i32 = 100; // Network is down
pub const ENETRESET: i32 = 102; // Network dropped connection on reset
pub const ECONNABORTED: i32 = 103; // Software caused connection abort
pub const ENOBUFS: i32 = 105; // No buffer space available
pub const ETXTBSY: i32 = 26; // Text file busy

// ---------------------------------------------------------------------------
// Per-thread errno storage
// ---------------------------------------------------------------------------

// Production: a single global atomic, suitable for the current
// single-threaded userspace runtime and matching the glibc/musl
// `*__errno_location()` ABI (which expects a stable address).  When
// our pthread layer can install real TLS slots this will move there.
//
// Test build: cargo runs tests in parallel threads but they all link
// into one process, so a single global would let one test's
// `set_errno(...)` clobber another test's `get_errno()` read mid-
// assertion.  Use a per-thread `Cell` in test builds so each test
// gets an isolated errno.
#[cfg(not(test))]
static ERRNO: AtomicI32 = AtomicI32::new(0);

#[cfg(test)]
std::thread_local! {
    static ERRNO_TLS: core::cell::Cell<i32> = const { core::cell::Cell::new(0) };
}

/// Set errno.
#[inline]
pub fn set_errno(val: i32) {
    #[cfg(not(test))]
    {
        ERRNO.store(val, Ordering::Relaxed);
    }
    #[cfg(test)]
    {
        ERRNO_TLS.with(|e| e.set(val));
    }
}

/// Get errno.
#[inline]
#[must_use]
pub fn get_errno() -> i32 {
    #[cfg(not(test))]
    {
        ERRNO.load(Ordering::Relaxed)
    }
    #[cfg(test)]
    {
        ERRNO_TLS.with(core::cell::Cell::get)
    }
}

/// C-compatible errno access.
///
/// Returns a pointer to the errno variable.  C programs access errno
/// via `*__errno_location()`.  This is the glibc/musl convention.
///
/// Not built for the test target — the TLS-backed test errno doesn't
/// expose a stable address, and no test references this function.
#[cfg(not(test))]
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

    // --- Process (200 range: -200 to -203) ---
    pub const NO_SUCH_PROCESS: i64 = -200;
    // InvalidExecutable = -201
    // ProcessExited = -202
    pub const NO_CHILD_PROCESS: i64 = -203;

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
        native::NO_CHILD_PROCESS => ECHILD,

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
        // TooManyLinks (-506) is the kernel's symlink-loop / max-symlink-depth
        // error (see kernel error.rs message "too many symbolic links" and its
        // sole producers — symlink-resolution depth checks + circular-symlink
        // detection in vfs/memfs/ext4, plus the O_NOFOLLOW final-symlink guard).
        // It maps to ELOOP, matching the Linux-ABI translation (linux.rs).
        // (No kernel path currently produces an EMLINK hard-link-count error; if
        // one is added it must use a distinct code, not this symlink error.)
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

    // In test builds errno is backed by a thread-local Cell, so each
    // test sees its own isolated errno value with no need for an
    // external mutex.  Production keeps the global AtomicI32.

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
    fn test_edeadlock_equals_edeadlk() {
        assert_eq!(EDEADLOCK, EDEADLK);
    }

    #[test]
    fn test_errno_extended_constants_match_linux() {
        // Linux-specific errno constants (44-57, 59)
        assert_eq!(ENOTBLK, 15);
        assert_eq!(ECHRNG, 44);
        assert_eq!(EL2NSYNC, 45);
        assert_eq!(EL3HLT, 46);
        assert_eq!(EL3RST, 47);
        assert_eq!(ELNRNG, 48);
        assert_eq!(EUNATCH, 49);
        assert_eq!(ENOCSI, 50);
        assert_eq!(EL2HLT, 51);
        assert_eq!(EBADE, 52);
        assert_eq!(EBADR, 53);
        assert_eq!(EXFULL, 54);
        assert_eq!(ENOANO, 55);
        assert_eq!(EBADRQC, 56);
        assert_eq!(EBADSLT, 57);
        assert_eq!(EBFONT, 59);
        // Network/remote (64-70, 73)
        assert_eq!(ENONET, 64);
        assert_eq!(ENOPKG, 65);
        assert_eq!(EREMOTE, 66);
        assert_eq!(EADV, 68);
        assert_eq!(ESRMNT, 69);
        assert_eq!(ECOMM, 70);
        assert_eq!(EDOTDOT, 73);
        // Shared library (76-83)
        assert_eq!(ENOTUNIQ, 76);
        assert_eq!(EBADFD, 77);
        assert_eq!(EREMCHG, 78);
        assert_eq!(ELIBACC, 79);
        assert_eq!(ELIBBAD, 80);
        assert_eq!(ELIBSCN, 81);
        assert_eq!(ELIBMAX, 82);
        assert_eq!(ELIBEXEC, 83);
        // System (85-87)
        assert_eq!(ERESTART, 85);
        assert_eq!(ESTRPIPE, 86);
        assert_eq!(EUSERS, 87);
        // Socket (94, 96)
        assert_eq!(ESOCKTNOSUPPORT, 94);
        assert_eq!(EPFNOSUPPORT, 96);
        // References/quota (109, 117-122, 124, 126-128)
        assert_eq!(ETOOMANYREFS, 109);
        assert_eq!(EUCLEAN, 117);
        assert_eq!(ENOTNAM, 118);
        assert_eq!(ENAVAIL, 119);
        assert_eq!(EISNAM, 120);
        assert_eq!(EREMOTEIO, 121);
        assert_eq!(EDQUOT, 122);
        assert_eq!(EMEDIUMTYPE, 124);
        assert_eq!(ENOKEY, 126);
        assert_eq!(EKEYEXPIRED, 127);
        assert_eq!(EKEYREVOKED, 128);
        assert_eq!(EKEYREJECTED, 129);
    }

    #[test]
    fn test_errno_values_no_duplicates() {
        // All distinct errno values (excluding aliases) must be unique.
        let vals: &[i32] = &[
            EPERM,
            ENOENT,
            ESRCH,
            EINTR,
            EIO,
            ENXIO,
            E2BIG,
            ENOEXEC,
            EBADF,
            ECHILD,
            EAGAIN,
            ENOMEM,
            EACCES,
            EFAULT,
            ENOTBLK,
            EBUSY,
            EEXIST,
            EXDEV,
            ENODEV,
            ENOTDIR,
            EISDIR,
            EINVAL,
            ENFILE,
            EMFILE,
            ENOTTY,
            ETXTBSY,
            EFBIG,
            ENOSPC,
            ESPIPE,
            EROFS,
            EMLINK,
            EPIPE,
            EDOM,
            ERANGE,
            EDEADLK,
            ENAMETOOLONG,
            ENOLCK,
            ENOSYS,
            ENOTEMPTY,
            ELOOP,
            ENOMSG,
            EIDRM,
            ECHRNG,
            EL2NSYNC,
            EL3HLT,
            EL3RST,
            ELNRNG,
            EUNATCH,
            ENOCSI,
            EL2HLT,
            EBADE,
            EBADR,
            EXFULL,
            ENOANO,
            EBADRQC,
            EBADSLT,
            EBFONT,
            ENOSTR,
            ENODATA,
            ETIME,
            ENOSR,
            ENONET,
            ENOPKG,
            EREMOTE,
            ENOLINK,
            EADV,
            ESRMNT,
            ECOMM,
            EPROTO,
            EMULTIHOP,
            EDOTDOT,
            EBADMSG,
            EOVERFLOW,
            ENOTUNIQ,
            EBADFD,
            EREMCHG,
            ELIBACC,
            ELIBBAD,
            ELIBSCN,
            ELIBMAX,
            ELIBEXEC,
            EILSEQ,
            ERESTART,
            ESTRPIPE,
            EUSERS,
            ENOTSOCK,
            EDESTADDRREQ,
            EMSGSIZE,
            EPROTOTYPE,
            ENOPROTOOPT,
            EPROTONOSUPPORT,
            ESOCKTNOSUPPORT,
            ENOTSUP,
            EPFNOSUPPORT,
            EAFNOSUPPORT,
            EADDRINUSE,
            EADDRNOTAVAIL,
            ENETDOWN,
            ENETUNREACH,
            ENETRESET,
            ECONNABORTED,
            ECONNRESET,
            ENOBUFS,
            EISCONN,
            ENOTCONN,
            ESHUTDOWN,
            ETOOMANYREFS,
            ETIMEDOUT,
            ECONNREFUSED,
            EHOSTDOWN,
            EHOSTUNREACH,
            EALREADY,
            EINPROGRESS,
            ESTALE,
            EUCLEAN,
            ENOTNAM,
            ENAVAIL,
            EISNAM,
            EREMOTEIO,
            EDQUOT,
            ENOMEDIUM,
            EMEDIUMTYPE,
            ECANCELED,
            ENOKEY,
            EKEYEXPIRED,
            EKEYREVOKED,
            EKEYREJECTED,
            EOWNERDEAD,
            ENOTRECOVERABLE,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(
                    vals[i], vals[j],
                    "errno values at indices {i} and {j} must be distinct"
                );
            }
        }
    }

    // __errno_location is only compiled outside of #[cfg(test)] (the
    // test errno backing storage is TLS, which has no stable address).
    // The function is exercised by the OS-target build at link time.

    #[test]
    fn test_translate_too_many_links() {
        // TOO_MANY_LINKS (-506) is the kernel's symlink-loop / max-symlink-depth
        // error (kernel message "too many symbolic links"; produced by symlink
        // resolution depth checks, circular-symlink detection, and the
        // O_NOFOLLOW final-symlink guard).  It maps to ELOOP, matching the
        // Linux-ABI translation.  (It is NOT the hard-link-count EMLINK error —
        // no kernel path produces that today.)
        let result = translate(native::TOO_MANY_LINKS);
        assert_eq!(result, -1);
        assert_eq!(get_errno(), ELOOP);
    }

    #[test]
    fn test_translate_filesystem_errors() {
        // Verify all filesystem error translations.
        assert_eq!(translate(native::NOT_FOUND), -1);
        assert_eq!(get_errno(), ENOENT);

        assert_eq!(translate(native::ALREADY_EXISTS), -1);
        assert_eq!(get_errno(), EEXIST);

        assert_eq!(translate(native::NOT_A_DIRECTORY), -1);
        assert_eq!(get_errno(), ENOTDIR);

        assert_eq!(translate(native::IS_A_DIRECTORY), -1);
        assert_eq!(get_errno(), EISDIR);

        assert_eq!(translate(native::NO_SPACE), -1);
        assert_eq!(get_errno(), ENOSPC);

        assert_eq!(translate(native::BAD_HANDLE), -1);
        assert_eq!(get_errno(), EBADF);

        assert_eq!(translate(native::DIRECTORY_NOT_EMPTY), -1);
        assert_eq!(get_errno(), ENOTEMPTY);

        assert_eq!(translate(native::READ_ONLY_FS), -1);
        assert_eq!(get_errno(), EROFS);

        assert_eq!(translate(native::TOO_MANY_OPEN_FILES), -1);
        assert_eq!(get_errno(), EMFILE);

        assert_eq!(translate(native::FILE_TOO_LARGE), -1);
        assert_eq!(get_errno(), EFBIG);
    }

    #[test]
    fn test_translate_ipc_errors() {
        assert_eq!(translate(native::CHANNEL_CLOSED), -1);
        assert_eq!(get_errno(), ECONNRESET);

        assert_eq!(translate(native::CHANNEL_FULL), -1);
        assert_eq!(get_errno(), EAGAIN);

        assert_eq!(translate(native::RESOURCE_EXHAUSTED), -1);
        assert_eq!(get_errno(), ENOMEM);
    }

    #[test]
    fn test_translate_device_errors() {
        assert_eq!(translate(native::IO_ERROR), -1);
        assert_eq!(get_errno(), EIO);

        assert_eq!(translate(native::NO_SUCH_DEVICE), -1);
        assert_eq!(get_errno(), ENODEV);

        assert_eq!(translate(native::RESOURCE_BUSY), -1);
        assert_eq!(get_errno(), EBUSY);
    }
}
