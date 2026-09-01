//! POSIX errno — per-thread error number.
//!
//! The POSIX errno convention: functions return -1 and set `errno` to
//! indicate which error occurred.  Our native syscalls return negative
//! error codes directly.  This module translates between the two.
//!
//! `errno` itself is per-thread, as POSIX requires: the storage lives in
//! [`crate::perthread`], inside the thread's own TLS mapping.

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

// `errno` is per-thread by definition — POSIX specifies it as a modifiable
// lvalue "each thread has its own", precisely so that one thread's failing
// `write` cannot be misread as another thread's.  The storage lives in
// [`crate::perthread`], which parks it in the thread's own TLS mapping; see
// that module for the layout and for the no-thread-pointer fallback.
//
// The value is a plain `i32`, not an atomic: only the owning thread ever
// reads or writes it, which is also what `__errno_location`'s ABI promises
// (it hands out a raw pointer that C code assigns through).

/// Set errno.
#[inline]
pub fn set_errno(val: i32) {
    // SAFETY: `perthread::current()` is non-null and valid for this thread,
    // and no other thread holds a pointer into this block.
    unsafe {
        (*crate::perthread::current()).errno = val;
    }
}

/// Get errno.
#[inline]
#[must_use]
pub fn get_errno() -> i32 {
    // SAFETY: as in `set_errno`.
    unsafe { (*crate::perthread::current()).errno }
}

/// C-compatible errno access.
///
/// Returns a pointer to the calling thread's errno.  C programs access
/// errno via `*__errno_location()`; this is the glibc/musl convention, and
/// the pointer stays valid until the thread exits.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __errno_location() -> *mut i32 {
    // `errno` is the first field of `PerThread` and the struct is
    // `repr(C)`, so the block pointer is also the errno pointer.
    crate::perthread::current().cast::<i32>()
}

// ---------------------------------------------------------------------------
// Native error code → POSIX errno translation
// ---------------------------------------------------------------------------

/// Our kernel error codes (from kernel/src/error.rs `KernelError` enum).
///
/// These are the negative values returned by native syscalls.
/// MUST stay in sync with kernel/src/error.rs — any mismatch causes
/// wrong errno values throughout the entire POSIX layer.
///
/// That sentence was a comment for a long time, and comments do not fail
/// builds.  Nine codes had been added to the kernel enum without reaching
/// here — `CrossDevice`, `StaleHandle`, and the whole `-700` network range —
/// and every one of them arrived in userspace as `EIO`, because the match
/// below ends in a catch-all.  `EIO` is not a value any caller acts on: a
/// non-blocking `connect` that should have said `EINPROGRESS` looked like a
/// dead socket rather than one still handshaking.  See
/// `kernel_error_codes_are_all_accounted_for` at the bottom of this file,
/// which reads `kernel/src/error.rs` and fails if a variant is missing from
/// this module or from [`errno_for`].  A number written down here on purpose
/// (as a commented-out line) satisfies it; a number nobody has looked at does
/// not.
pub(crate) mod native {
    // --- General (0-99 range: -1 to -8) ---
    pub const INTERNAL_ERROR: i64 = -1;
    pub const NOT_SUPPORTED: i64 = -2;
    pub const INVALID_ARGUMENT: i64 = -3;
    pub const WOULD_BLOCK: i64 = -4;
    pub const CANCELLED: i64 = -5;
    pub const TIMED_OUT: i64 = -6;
    pub const DEADLOCK: i64 = -7;
    /// A blocking syscall was interrupted by a signal.
    ///
    /// This became reachable on the native ABI when `SYS_TTY_READ` (543)
    /// started returning it: a `^C` during a console read whose handler the
    /// program installed itself resolves to `KernelError::Interrupted`.
    /// Before it was mapped here it fell through to `EIO`, which is not a
    /// value any `read()` caller retries on.  Note the number: Linux's
    /// `EINTR` is 4, and `-4` here is `WOULD_BLOCK`, so the two ABIs must
    /// never share an encoding path.
    pub const INTERRUPTED: i64 = -8;
    /// A caller-supplied output buffer was too small to hold the whole answer.
    ///
    /// Deliberately distinct from `INVALID_ARGUMENT`: the request was
    /// well-formed and the kernel *could* have answered it, so the caller's
    /// correct response is to allocate more and retry rather than give up. A
    /// handler returning this writes **nothing** — which is the point of the
    /// variant, because for an enumeration like `SYS_CAP_QUERY` a silently
    /// truncated prefix would read as "this process holds less authority than
    /// it does": the same class of bug as over-reporting, in the direction that
    /// is harder to notice.
    ///
    /// Added by lane A alongside `SYS_CAP_QUERY`'s enumerate mode
    /// (`requests/a-b-cap-query-enumeration-landed.md`). Until it was mapped
    /// here it fell through to the catch-all `EIO`, which no caller retries on.
    pub const BUFFER_TOO_SMALL: i64 = -9;
    /// The syscall number names an **empty slot** — the kernel has never heard
    /// of it.
    ///
    /// This is the fact [`NOT_SUPPORTED`] used to have to carry as well, and
    /// the two are acted on differently by the caller: "the kernel is older
    /// than this call, take the previous route" against "the call ran and this
    /// filesystem cannot do the thing, stop". While both were `-2` the
    /// difference could only be guessed at, and [`crate::file::pinned_answer`]
    /// did the guessing with a per-syscall latch — sound, but wrong for exactly
    /// as long as no answer had yet arrived, which silently downgraded a
    /// genuine first-call refusal to the racy path-based route.
    ///
    /// `ENOSYS`, which is also what `linux_errno_for` gives `NotSupported`:
    /// Linux has one errno for both facts, so only the native ABI can tell them
    /// apart, and only callers on the native ABI need to.
    ///
    /// Added by lane A in `dispatch.rs`'s unregistered-slot arm.
    pub const NO_SUCH_SYSCALL: i64 = -10;

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
    /// A handle was presented that does not name a capability this process
    /// holds.  `EACCES`, the same as an outright denial: from the caller's
    /// side the two are one fact — the operation was not permitted — and
    /// distinguishing them would tell an unprivileged caller which handles
    /// exist.
    pub const INVALID_CAPABILITY: i64 = -401;

    // --- Filesystem (500 range: -500 to -513) ---
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
    /// An operation needing both operands on one filesystem crossed a mount
    /// boundary — `RENAME_EXCHANGE` between mounts, a hard link spanning two.
    /// `EXDEV`, which is the error `mv` reads to decide it must fall back to
    /// copy-then-delete; as `EIO` it looked like a failing disk instead.
    pub const CROSS_DEVICE: i64 = -512;
    /// A directory handle no longer denotes the directory it was opened on.
    /// Returned by the pinned `*at` calls (662-664), which verify the
    /// `(fs_id, inode)` captured at open before acting.  `ESTALE` means
    /// *re-open*, not *retry* — a caller that retries the same handle gets
    /// the same answer forever.
    pub const STALE_HANDLE: i64 = -513;
    /// The *attribute name* is what is missing, as opposed to [`NOT_FOUND`],
    /// where the *path* is.  The two shared `NotFound` until lane A's
    /// `32f35d46b` split them, and the conflation had a cost: `cp
    /// --preserve=all` cannot tell "this file has no `user.foo`" — which is the
    /// ordinary case and must be silent — from "this file is gone", which is a
    /// diagnostic.  `ENODATA` is the errno Linux returns from `getxattr(2)` for
    /// it; `ENOATTR` is the same number under its BSD name.
    pub const NO_ATTRIBUTE: i64 = -514;

    // --- Device / I/O (600 range: -600 to -602) ---
    pub const IO_ERROR: i64 = -600;
    pub const NO_SUCH_DEVICE: i64 = -601;
    pub const RESOURCE_BUSY: i64 = -602;

    // --- Network (700 range: -700 to -706) ---
    //
    // This whole range exists to be translated -- each variant's kernel doc
    // comment names the errno it is for -- and none of it was, until
    // 2026-08-30.  `EINPROGRESS` and `EALREADY` are the ones that mattered
    // most: they are not failures at all, they are how a non-blocking
    // `connect` reports that the handshake is still running, and a caller
    // that sees `EIO` there gives up on a socket that was about to connect.
    pub const CONNECTION_REFUSED: i64 = -700;
    pub const NOT_CONNECTED: i64 = -701;
    pub const IN_PROGRESS: i64 = -702;
    pub const CONNECT_ALREADY: i64 = -703;
    pub const BROKEN_PIPE: i64 = -704;
    pub const ADDR_IN_USE: i64 = -705;
    pub const MSG_SIZE: i64 = -706;
}

/// The errno a native kernel error code corresponds to.
///
/// Split out of [`translate`] so there is exactly one such table in the
/// library.  There used to be two: `socket.rs` carried its own
/// `translate_net_error`, which differed from this one in two rows and was
/// missing eighteen — including every code in the `-700` range, which exists
/// for sockets and nothing else.  Two hand-maintained mirrors of one kernel
/// enum drift, and the second one drifted silently because its catch-all
/// answers `EIO` for anything it has not heard of.  `translate_net_error`
/// now delegates here and keeps only the one row that is genuinely
/// socket-specific (`AlreadyExists` means a bind collision, not `EEXIST`).
///
/// `code` is expected to be negative; a non-negative value is not an error
/// and callers should not reach here with one.  It is still answered, with
/// `EIO`, rather than panicking — this runs under every syscall wrapper in
/// the process and a panic there would turn a wrong return value into a
/// dead program.
#[must_use]
pub fn errno_for(code: i64) -> i32 {
    #[allow(clippy::match_same_arms)] // Kept separate for readability: each
    // native error code documents its semantic mapping even when the POSIX
    // target is the same (e.g. INTERNAL_ERROR and IO_ERROR both → EIO).
    match code {
        // General errors
        native::INTERNAL_ERROR => EIO,
        native::NOT_SUPPORTED => ENOTSUP,
        native::INVALID_ARGUMENT => EINVAL,
        native::WOULD_BLOCK | native::CHANNEL_FULL => EAGAIN,
        native::CANCELLED => ECANCELED,
        native::TIMED_OUT => ETIMEDOUT,
        native::DEADLOCK => EDEADLK,
        native::INTERRUPTED => EINTR,
        // ERANGE, not EINVAL: the caller should retry with a bigger buffer.
        native::BUFFER_TOO_SMALL => ERANGE,
        // ENOSYS — "function not implemented" — which is what POSIX has always
        // said an unimplemented call should report, and which an unwired slot
        // could not report while it shared `-2` with `NOT_SUPPORTED` and so
        // arrived as `ENOTSUP`. So this is a small user-visible improvement as
        // well as an internal one: a caller probing for a syscall's existence
        // now gets the errno it is written to test for.
        native::NO_SUCH_SYSCALL => ENOSYS,

        // Memory errors
        native::OUT_OF_MEMORY | native::RESOURCE_EXHAUSTED => ENOMEM,
        native::INVALID_ADDRESS => EFAULT,

        // Process errors
        native::NO_SUCH_PROCESS => ESRCH,
        native::NO_CHILD_PROCESS => ECHILD,

        // IPC errors
        native::CHANNEL_CLOSED => ECONNRESET,

        // Capability / permission errors
        native::PERMISSION_DENIED | native::INVALID_CAPABILITY => EACCES,

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
        native::CROSS_DEVICE => EXDEV,
        native::STALE_HANDLE => ESTALE,
        native::NO_ATTRIBUTE => ENODATA,

        // Device / I/O errors
        native::IO_ERROR => EIO,
        native::NO_SUCH_DEVICE => ENODEV,
        native::RESOURCE_BUSY => EBUSY,

        // Network errors
        native::CONNECTION_REFUSED => ECONNREFUSED,
        native::NOT_CONNECTED => ENOTCONN,
        native::IN_PROGRESS => EINPROGRESS,
        native::CONNECT_ALREADY => EALREADY,
        native::BROKEN_PIPE => EPIPE,
        native::ADDR_IN_USE => EADDRINUSE,
        native::MSG_SIZE => EMSGSIZE,

        // Unknown error → generic I/O error.
        //
        // This arm is why the accounting test at the bottom of the file
        // exists.  It cannot distinguish "a code the kernel does not define"
        // from "a code the kernel defines and this table has not caught up
        // with", so on its own it converts a missing row into a plausible
        // wrong answer rather than a loud one.  The test supplies the
        // distinction the arm cannot.
        _ => EIO,
    }
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
    set_errno(errno_for(ret));
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // errno is per-thread on every build (see `crate::perthread`), so tests
    // running in parallel on the harness's thread pool cannot clobber each
    // other's value and need no external mutex.

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
    fn test_translate_interrupted() {
        // A blocking native syscall interrupted by a signal.  Reachable since
        // SYS_TTY_READ (543): a ^C during a console read, with a handler
        // installed, resolves to KernelError::Interrupted rather than a
        // restart.  Until it was mapped this fell through to EIO, which no
        // read() caller retries on.
        assert_eq!(translate(native::INTERRUPTED), -1);
        assert_eq!(get_errno(), EINTR);
    }

    #[test]
    fn test_native_interrupted_is_not_would_block() {
        // The two ABIs number these differently and must never share an
        // encoding path: Linux's EINTR is 4, while the *native* -4 is
        // WouldBlock.  A kernel path that substituted a Linux EINTR into a
        // native return value would silently report EAGAIN for "interrupted".
        assert_ne!(native::INTERRUPTED, native::WOULD_BLOCK);
        assert_eq!(native::INTERRUPTED, -8);
        assert_eq!(-i64::from(EINTR), native::WOULD_BLOCK);
        assert_eq!(translate(native::WOULD_BLOCK), -1);
        assert_eq!(get_errno(), EAGAIN);
    }

    #[test]
    fn test_translate_deadlock() {
        assert_eq!(translate(native::DEADLOCK), -1);
        assert_eq!(get_errno(), EDEADLK);
    }

    #[test]
    fn test_translate_buffer_too_small_is_erange_not_einval() {
        // ERANGE is the retryable answer: the request was well-formed and the
        // caller should allocate more and ask again.  EINVAL would say the
        // request itself was wrong, and EIO — where this landed before it was
        // mapped, via the catch-all arm — says nothing a caller can act on.
        assert_eq!(translate(native::BUFFER_TOO_SMALL), -1);
        assert_eq!(get_errno(), ERANGE);
        assert_ne!(ERANGE, EINVAL);
        assert_ne!(ERANGE, EIO);
    }

    #[test]
    fn test_native_buffer_too_small_matches_kernel_error_rs() {
        // This module is a hand-maintained mirror of kernel/src/error.rs and
        // nothing but a test enforces that.  -9 is `KernelError::BufferTooSmall`
        // and is what SYS_CAP_QUERY's enumerate mode returns when the caller's
        // array cannot hold every entry; it must not collide with the general
        // errors either side of it.
        assert_eq!(native::BUFFER_TOO_SMALL, -9);
        assert_ne!(native::BUFFER_TOO_SMALL, native::INTERRUPTED);
        assert_ne!(native::BUFFER_TOO_SMALL, native::INVALID_ARGUMENT);
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

    #[test]
    fn test_translate_cross_device_and_stale_handle() {
        // -512 is what `mv` reads to decide a rename must become
        // copy-then-delete, and -513 is what the pinned `*at` calls return
        // when a directory handle no longer denotes its directory.  Both
        // reached userspace as EIO until 2026-08-30.
        assert_eq!(translate(native::CROSS_DEVICE), -1);
        assert_eq!(get_errno(), EXDEV);

        assert_eq!(translate(native::STALE_HANDLE), -1);
        assert_eq!(get_errno(), ESTALE);
    }

    #[test]
    fn test_translate_network_errors() {
        for (code, want) in [
            (native::CONNECTION_REFUSED, ECONNREFUSED),
            (native::NOT_CONNECTED, ENOTCONN),
            (native::IN_PROGRESS, EINPROGRESS),
            (native::CONNECT_ALREADY, EALREADY),
            (native::BROKEN_PIPE, EPIPE),
            (native::ADDR_IN_USE, EADDRINUSE),
            (native::MSG_SIZE, EMSGSIZE),
        ] {
            assert_eq!(translate(code), -1, "code {code} should be an error");
            assert_eq!(get_errno(), want, "code {code} mapped to the wrong errno");
        }
    }

    // -----------------------------------------------------------------
    // The accounting check
    // -----------------------------------------------------------------
    //
    // `native` above is a hand-written mirror of `KernelError` in another
    // crate, and `errno_for` is a hand-written match over it.  The header
    // said "MUST stay in sync" and nothing enforced it, so nine codes were
    // added to the kernel and never arrived here.  They did not fail
    // loudly, because `errno_for` ends in a catch-all: they became `EIO`,
    // which is a legitimate answer for several real codes and therefore
    // looks like a mapping rather than the absence of one.
    //
    // This reads the kernel's enum and requires each variant to be
    // *accounted for* — either mapped, or written down here as a
    // deliberately-unmapped number.  It is a source-text check because the
    // posix crate cannot depend on the kernel crate (different target, and
    // `no_std` in the other direction), so there is no type to reflect on.
    // Being textual, it is deliberately generous about what it accepts: any
    // occurrence of the number inside the `native` block counts, including
    // in a comment.  The bar is that a human wrote the number down, not
    // that they wrote it down in a particular shape.
    //
    // With one exception, found by deleting `STALE_HANDLE` and watching the
    // check pass anyway.  The section dividers name their ranges as
    // `// --- Filesystem (500 range: -500 to -513) ---`, so a divider
    // permanently satisfies the two codes at its own endpoints — which are
    // exactly the codes most likely to be the new ones, since a range grows
    // at its end.  Dividers are therefore excluded from the searched text.
    // Nothing else is: a genuine acknowledgment like
    // `// PageFault = -102 (not typically returned to userspace)` still
    // counts, which is the point of accepting comments at all.

    /// Every `Name = -NNN,` in the kernel's `KernelError` enum.
    fn kernel_error_codes() -> Vec<(String, i64)> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/src/error.rs");
        // Fail rather than skip.  A check that quietly passes when it could
        // not run is worse than no check: it reports the state of the world
        // it did not look at.
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("cannot read the kernel error enum at {path}: {e}");
        });
        let mut out = Vec::new();
        for line in src.lines() {
            let line = line.trim();
            let Some((name, rest)) = line.split_once(" = -") else {
                continue;
            };
            let Some(digits) = rest.strip_suffix(',') else {
                continue;
            };
            if !name.chars().all(|c| c.is_ascii_alphanumeric()) || name.is_empty() {
                continue;
            }
            if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            let Ok(value) = digits.parse::<i64>() else {
                continue;
            };
            out.push((name.to_string(), -value));
        }
        assert!(
            out.len() > 30,
            "only found {} enum variants in {path}; the parse is probably \
             wrong rather than the enum being tiny",
            out.len()
        );
        out
    }

    /// The text of a named region of this very file.
    fn own_source_between(open: &str, close: &str) -> String {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/errno.rs"))
            .expect("cannot read errno.rs; the check would otherwise pass vacuously");
        let start = src
            .find(open)
            .unwrap_or_else(|| panic!("no {open:?} in errno.rs"));
        let rest = &src[start..];
        let end = rest
            .find(close)
            .unwrap_or_else(|| panic!("no {close:?} after {open:?}"));
        rest[..end].to_string()
    }

    #[test]
    fn kernel_error_codes_are_all_accounted_for() {
        let block: String = own_source_between("pub(crate) mod native {", "\n}\n")
            .lines()
            .filter(|l| !l.trim_start().starts_with("// ---"))
            .collect::<Vec<_>>()
            .join("\n");
        let missing: Vec<String> = kernel_error_codes()
            .into_iter()
            .filter(|(_, value)| {
                // Match on the exact token, so that `-50` does not satisfy
                // `-500` and `-70` does not satisfy `-700`.
                let needle = format!("{value}");
                !block
                    .split(|c: char| !(c.is_ascii_digit() || c == '-'))
                    .any(|t| t == needle)
            })
            .map(|(name, value)| format!("{name} ({value})"))
            .collect();
        assert!(
            missing.is_empty(),
            "kernel/src/error.rs defines {} error code(s) that `mod native` in \
             posix/src/errno.rs has never heard of:\n  {}\n\
             Each one currently reaches userspace as EIO. Add a `pub const` and \
             an `errno_for` arm, or -- if it genuinely never crosses the syscall \
             boundary -- a commented-out `// Name = -NNN` line saying so.",
            missing.len(),
            missing.join("\n  "),
        );
    }

    #[test]
    fn every_declared_code_is_actually_translated() {
        let block = own_source_between("pub(crate) mod native {", "\n}\n");
        let body = own_source_between("pub fn errno_for(code: i64) -> i32 {", "\n}\n");
        let mut unmapped = Vec::new();
        for line in block.lines() {
            let Some(rest) = line.trim().strip_prefix("pub const ") else {
                continue;
            };
            let Some((name, _)) = rest.split_once(':') else {
                continue;
            };
            if !body.contains(&format!("native::{name}")) {
                unmapped.push(name.to_string());
            }
        }
        assert!(
            unmapped.is_empty(),
            "these `native` constants are declared but never consulted by \
             `errno_for`, so they translate to EIO through the catch-all: {}",
            unmapped.join(", "),
        );
    }
}
