//! POSIX unistd.h equivalents — miscellaneous functions.
//!
//! Functions that don't fit neatly into another category: `getcwd`,
//! `chdir`, `isatty`, `getuid`, `getgid`, `sysconf`, `daemon`,
//! `getloadavg`, `write` to stdout/stderr.
//!
//! ## Current Working Directory
//!
//! CWD is tracked purely in userspace via a static buffer per process
//! (each process has its own address space).  `chdir()` validates the
//! target via `SYS_FS_STAT` and stores the normalized absolute path.
//! `getcwd()` copies from this buffer.  `resolve_path()` is the public
//! API used by all file-operation functions (`open`, `stat`, `unlink`,
//! etc.) to resolve relative paths before passing them to the kernel.
//!
//! ## Path Normalization
//!
//! `normalize_path()` handles `.`, `..`, redundant `/`, and trailing
//! slashes.  `..` at root is a no-op (cannot ascend above `/`).

use crate::errno;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Standard file descriptors
// ---------------------------------------------------------------------------

/// Standard input.
pub const STDIN_FILENO: Fd = 0;
/// Standard output.
pub const STDOUT_FILENO: Fd = 1;
/// Standard error.
pub const STDERR_FILENO: Fd = 2;

// ---------------------------------------------------------------------------
// POSIX feature-test macros (exported as constants for C programs)
// ---------------------------------------------------------------------------

/// POSIX version this implementation conforms to (POSIX.1-2008).
#[allow(non_upper_case_globals)]
pub const _POSIX_VERSION: i64 = 200_809;

/// XSI (X/Open System Interfaces) extension version.
#[allow(non_upper_case_globals)]
pub const _XOPEN_VERSION: i32 = 700;

/// Threads are supported (value = POSIX version).
#[allow(non_upper_case_globals)]
pub const _POSIX_THREADS: i64 = 200_809;

/// Memory-mapped files are supported.
#[allow(non_upper_case_globals)]
pub const _POSIX_MAPPED_FILES: i64 = 200_809;

/// Memory protection is supported.
#[allow(non_upper_case_globals)]
pub const _POSIX_MEMORY_PROTECTION: i64 = 200_809;

/// fsync is supported.
#[allow(non_upper_case_globals)]
pub const _POSIX_FSYNC: i64 = 200_809;

/// Timers are supported (value = POSIX version).
#[allow(non_upper_case_globals)]
pub const _POSIX_TIMERS: i64 = 200_809;

/// Monotonic clock is available.
#[allow(non_upper_case_globals)]
pub const _POSIX_MONOTONIC_CLOCK: i64 = 200_809;

/// Clock selection is available.
#[allow(non_upper_case_globals)]
pub const _POSIX_CLOCK_SELECTION: i64 = 200_809;

/// Saved set-user-ID and set-group-ID are supported.
#[allow(non_upper_case_globals)]
pub const _POSIX_SAVED_IDS: i32 = 1;

/// Job control is supported.
#[allow(non_upper_case_globals)]
pub const _POSIX_JOB_CONTROL: i32 = 1;

// ---------------------------------------------------------------------------
// sysconf names
// ---------------------------------------------------------------------------

/// Page size (sysconf name).
pub const _SC_PAGESIZE: i32 = 30;
/// Page size (alias).
pub const _SC_PAGE_SIZE: i32 = _SC_PAGESIZE;
/// Number of configured processors.
pub const _SC_NPROCESSORS_CONF: i32 = 83;
/// Number of online processors.
pub const _SC_NPROCESSORS_ONLN: i32 = 84;
/// Open max (max file descriptors).
pub const _SC_OPEN_MAX: i32 = 4;
/// Clock ticks per second.
pub const _SC_CLK_TCK: i32 = 2;
/// Maximum length of arguments to exec.
pub const _SC_ARG_MAX: i32 = 0;
/// Maximum number of child processes per user.
pub const _SC_CHILD_MAX: i32 = 1;
/// Maximum number of supplementary groups.
pub const _SC_NGROUPS_MAX: i32 = 3;
/// POSIX version (200809L = POSIX.1-2008).
pub const _SC_VERSION: i32 = 29;
/// Maximum length of a hostname.
pub const _SC_HOST_NAME_MAX: i32 = 180;
/// Maximum length of a login name.
pub const _SC_LOGIN_NAME_MAX: i32 = 71;
/// Maximum line length.
pub const _SC_LINE_MAX: i32 = 43;
/// Whether POSIX threads are supported.
pub const _SC_THREADS: i32 = 67;
/// Minimum stack size for a thread.
pub const _SC_THREAD_STACK_MIN: i32 = 75;
/// Total physical memory pages.
pub const _SC_PHYS_PAGES: i32 = 85;
/// Available physical memory pages.
pub const _SC_AVPHYS_PAGES: i32 = 86;
/// Maximum number of iovec entries for readv/writev.
pub const _SC_IOV_MAX: i32 = 60;

/// Suggested size for getpwnam_r/getpwuid_r buffers.
pub const _SC_GETPW_R_SIZE_MAX: i32 = 70;
/// Suggested size for getgrnam_r/getgrgid_r buffers.
pub const _SC_GETGR_R_SIZE_MAX: i32 = 69;
/// Maximum symlink resolution depth.
pub const _SC_SYMLOOP_MAX: i32 = 173;
/// Maximum number of open streams per process.
pub const _SC_STREAM_MAX: i32 = 5;
/// TTY name max length.
pub const _SC_TTY_NAME_MAX: i32 = 72;
/// Maximum RE_DUP repetition count.
pub const _SC_RE_DUP_MAX: i32 = 44;
/// Maximum number of bytes for a timezone name.
pub const _SC_TZNAME_MAX: i32 = 6;
/// Maximum POSIX message queues per process.
pub const _SC_MQ_OPEN_MAX: i32 = 27;
/// Maximum message queue priority.
pub const _SC_MQ_PRIO_MAX: i32 = 28;
/// Maximum semaphore value.
pub const _SC_SEM_VALUE_MAX: i32 = 33;
/// Maximum timers per process.
pub const _SC_TIMER_MAX: i32 = 35;
/// Maximum `ibase`/`obase` for `bc`.
pub const _SC_BC_BASE_MAX: i32 = 36;
/// Maximum array elements for `bc`.
pub const _SC_BC_DIM_MAX: i32 = 37;
/// Maximum `scale` for `bc`.
pub const _SC_BC_SCALE_MAX: i32 = 38;
/// Maximum string length for `bc`.
pub const _SC_BC_STRING_MAX: i32 = 39;
/// Maximum collation weights per character.
pub const _SC_COLL_WEIGHTS_MAX: i32 = 40;
/// Maximum `expr` nesting depth.
pub const _SC_EXPR_NEST_MAX: i32 = 42;
/// POSIX.2 version.
#[allow(non_upper_case_globals)]
pub const _SC_2_VERSION: i32 = 46;
/// POSIX.2 C language binding.
#[allow(non_upper_case_globals)]
pub const _SC_2_C_BIND: i32 = 47;
/// Maximum number of thread destructor iterations.
pub const _SC_THREAD_DESTRUCTOR_ITERATIONS: i32 = 73;
/// Maximum concurrent threads per process.
pub const _SC_THREAD_THREADS_MAX: i32 = 74;
/// Maximum number of thread-specific data keys.
pub const _SC_THREAD_KEYS_MAX: i32 = 76;

// ---------------------------------------------------------------------------
// Current working directory tracking
// ---------------------------------------------------------------------------

/// Maximum path length (POSIX `PATH_MAX`).
///
/// Bounds the CWD buffer and all resolved absolute paths returned by
/// [`resolve_path`].
pub const PATH_MAX: usize = 4096;

/// Current working directory buffer.
///
/// Each userspace process gets its own copy via separate virtual
/// address spaces.  Initialized to "/" (root filesystem).
///
/// Invariant: always contains a normalized absolute path of length
/// `CWD_LEN` (no null terminator stored).
static mut CWD_BUF: [u8; PATH_MAX] = {
    let mut buf = [0u8; PATH_MAX];
    buf[0] = b'/';
    buf
};

/// Length of the CWD string (excludes any null terminator).
static mut CWD_LEN: usize = 1;

/// Raw pointer to the CWD buffer (avoids direct `static mut` reference).
#[inline]
fn cwd_buf_ptr() -> *mut [u8; PATH_MAX] {
    core::ptr::addr_of_mut!(CWD_BUF)
}

/// Raw pointer to the CWD length.
#[inline]
fn cwd_len_ptr() -> *mut usize {
    core::ptr::addr_of_mut!(CWD_LEN)
}

// ---------------------------------------------------------------------------
// Path normalization
// ---------------------------------------------------------------------------

/// Normalize an absolute path by resolving `.`, `..`, and redundant `/`.
///
/// `input` must begin with `b'/'`.  The normalized result is written
/// to `out` (no null terminator) and the byte length is returned.
///
/// Returns `None` if `input` is not absolute or the result exceeds
/// `out.len()`.
///
/// Guarantees on the output:
/// - Starts with `/`.
/// - No trailing `/` (except root `/`).
/// - No `//`, `/./`, or `/../` sequences.
/// - `..` at root is a no-op (cannot ascend above `/`).
fn normalize_path(input: &[u8], out: &mut [u8]) -> Option<usize> {
    if input.first() != Some(&b'/') {
        return None;
    }

    let in_len = input.len();
    let out_cap = out.len();
    let mut out_len: usize = 0;
    let mut i: usize = 0;

    while i < in_len {
        // Skip consecutive slashes.
        while i < in_len && input.get(i) == Some(&b'/') {
            i = i.wrapping_add(1);
        }
        if i >= in_len {
            break;
        }

        // Delimit the current component.
        let start = i;
        while i < in_len && input.get(i) != Some(&b'/') {
            i = i.wrapping_add(1);
        }
        let comp_len = i.wrapping_sub(start);

        // "." — current directory, skip entirely.
        if comp_len == 1 && input.get(start) == Some(&b'.') {
            continue;
        }

        // ".." — parent directory, pop the last component.
        if comp_len == 2
            && input.get(start) == Some(&b'.')
            && input.get(start.wrapping_add(1)) == Some(&b'.')
        {
            while out_len > 0 {
                out_len = out_len.wrapping_sub(1);
                if out.get(out_len) == Some(&b'/') {
                    break;
                }
            }
            continue;
        }

        // Normal component: append "/name".
        let needed = out_len.wrapping_add(1).wrapping_add(comp_len);
        if needed > out_cap {
            return None;
        }

        if let Some(slot) = out.get_mut(out_len) {
            *slot = b'/';
        }
        out_len = out_len.wrapping_add(1);

        for j in 0..comp_len {
            if let (Some(dst), Some(&src)) = (
                out.get_mut(out_len),
                input.get(start.wrapping_add(j)),
            ) {
                *dst = src;
                out_len = out_len.wrapping_add(1);
            }
        }
    }

    // Empty output means we collapsed everything back to root.
    if out_len == 0 {
        if let Some(slot) = out.get_mut(0) {
            *slot = b'/';
        }
        out_len = 1;
    }

    Some(out_len)
}

/// Resolve a C-string path against the current working directory.
///
/// - Absolute paths (starting with `/`) are normalized in place.
/// - Relative paths are prepended with the CWD before normalization.
///
/// The result is written to `out` (no null terminator) and the byte
/// count is returned.  Returns `None` when `path` is null, empty, or
/// the resolved result exceeds [`PATH_MAX`].
///
/// # Safety
///
/// `path` must point to a valid null-terminated C string.
pub unsafe fn resolve_path(path: *const u8, out: &mut [u8; PATH_MAX]) -> Option<usize> {
    if path.is_null() {
        return None;
    }

    // SAFETY: Caller guarantees `path` is a valid C string.
    let path_len = unsafe { crate::string::strlen(path) };
    if path_len == 0 {
        return None;
    }

    // SAFETY: `strlen` guarantees `path` is readable for `path_len` bytes.
    let first = unsafe { *path };

    if first == b'/' {
        // Absolute path — normalize directly.
        let slice = unsafe { core::slice::from_raw_parts(path, path_len) };
        normalize_path(slice, out)
    } else {
        // Relative path — prepend CWD, then normalize.
        let mut combined = [0u8; PATH_MAX];

        // SAFETY: Single-threaded per-process access to CWD state.
        let cwd_len = unsafe { *cwd_len_ptr() };
        if cwd_len >= PATH_MAX {
            return None;
        }
        // SAFETY: cwd_buf_ptr() is valid for PATH_MAX bytes; cwd_len <= PATH_MAX.
        let cwd = unsafe {
            core::slice::from_raw_parts(cwd_buf_ptr().cast::<u8>(), cwd_len)
        };

        // Copy CWD into the combined buffer.
        let mut pos: usize = 0;
        for idx in 0..cwd_len {
            if let (Some(&b), Some(slot)) = (cwd.get(idx), combined.get_mut(pos)) {
                *slot = b;
                pos = pos.wrapping_add(1);
            }
        }

        // Append separator unless CWD already ends with '/'.
        let last_is_slash = pos > 0
            && combined.get(pos.wrapping_sub(1)) == Some(&b'/');
        if !last_is_slash {
            if pos >= PATH_MAX {
                return None;
            }
            if let Some(slot) = combined.get_mut(pos) {
                *slot = b'/';
            }
            pos = pos.wrapping_add(1);
        }

        // Append the relative path.
        let rel = unsafe { core::slice::from_raw_parts(path, path_len) };
        for idx in 0..path_len {
            if pos >= PATH_MAX {
                return None;
            }
            if let (Some(&b), Some(slot)) = (rel.get(idx), combined.get_mut(pos)) {
                *slot = b;
                pos = pos.wrapping_add(1);
            }
        }

        match combined.get(..pos) {
            Some(slice) => normalize_path(slice, out),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Get the current working directory.
///
/// Copies the absolute pathname of the CWD into `buf` (null-terminated).
/// Returns `buf` on success, null on error with errno set.
///
/// # Errors
///
/// - `EINVAL` — `buf` is null or `size` is 0.
/// - `ERANGE` — `size` is too small for the CWD path plus its null
///   terminator.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getcwd(buf: *mut u8, size: SizeT) -> *mut u8 {
    if buf.is_null() || size == 0 {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }

    // SAFETY: Single-threaded per-process access to CWD state.
    let cwd_len = unsafe { *cwd_len_ptr() };

    // Need room for the path string plus a null terminator.
    let needed = cwd_len.wrapping_add(1);
    if size < needed {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    }

    // SAFETY: CWD buffer is valid for `cwd_len` bytes; `buf` is valid
    // for at least `size` bytes (caller contract).
    unsafe {
        let cwd = core::slice::from_raw_parts(cwd_buf_ptr().cast::<u8>(), cwd_len);
        for i in 0..cwd_len {
            if let Some(&b) = cwd.get(i) {
                *buf.add(i) = b;
            }
        }
        *buf.add(cwd_len) = 0;
    }

    buf
}

/// Change the current working directory.
///
/// Resolves `path` against the current CWD (if relative), verifies
/// that the target exists and is a directory, then stores the
/// normalized absolute path as the new CWD.
///
/// Returns 0 on success, -1 on error with errno set.
///
/// # Errors
///
/// - `EFAULT` — `path` is null.
/// - `ENOENT` — `path` is empty or does not exist.
/// - `ENOTDIR` — resolved path exists but is not a directory.
/// - `ENAMETOOLONG` — resolved path exceeds `PATH_MAX`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chdir(path: *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: `path` is a valid C string (caller contract).
    let path_len = unsafe { crate::string::strlen(path) };
    if path_len == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    // Resolve relative paths and normalize.
    let mut resolved = [0u8; PATH_MAX];
    let Some(resolved_len) = (unsafe { resolve_path(path, &mut resolved) }) else {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    };

    // Verify the target exists and is a directory.
    let mut stat_buf = core::mem::MaybeUninit::<crate::stat::Stat>::zeroed();
    let ret = syscall3(
        SYS_FS_STAT,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        stat_buf.as_mut_ptr() as u64,
    );

    if ret < 0 {
        return errno::translate(ret) as i32;
    }

    // SAFETY: Kernel wrote a valid Stat into the buffer.
    let sb = unsafe { stat_buf.assume_init() };
    if !sb.is_dir() {
        errno::set_errno(errno::ENOTDIR);
        return -1;
    }

    // Store as the new CWD.
    // SAFETY: Single-threaded per-process access.
    unsafe {
        let buf = &mut *cwd_buf_ptr();
        for i in 0..resolved_len {
            if let (Some(dst), Some(&src)) = (buf.get_mut(i), resolved.get(i)) {
                *dst = src;
            }
        }
        *cwd_len_ptr() = resolved_len;
    }

    0
}

/// Change working directory by file descriptor.
///
/// Looks up the absolute path stored at open time for `fd` (see
/// [`crate::fdtable::store_fd_path`]) and delegates to [`chdir()`].
///
/// This works because `open()` records the resolved path for every fd.
/// If the fd has no stored path (e.g., a pipe or socket), returns
/// `ENOTDIR`.  If the stored path is no longer a valid directory
/// (e.g., it was renamed or removed), `chdir()` will report the error.
///
/// **Limitation:** if the directory is renamed after the fd is opened,
/// the stored path becomes stale.  Real kernels track the dentry
/// directly and follow renames; we track the path string instead.
///
/// # Errors
///
/// - `EBADF` — `fd` is not a valid open file descriptor.
/// - `ENOTDIR` — `fd` does not refer to a directory (or has no path).
/// - Other errors from `chdir()` (e.g., `ENOENT` if the path is stale).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchdir(fd: Fd) -> i32 {
    // Verify the fd is valid.
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // Look up the stored path.
    let mut path_buf = [0u8; PATH_MAX];
    let path_len = crate::fdtable::get_fd_path(fd, &mut path_buf);
    if path_len == 0 {
        // No path stored — fd is a pipe, socket, or was not opened
        // through our open() (e.g., stdin/stdout/stderr console fds).
        errno::set_errno(errno::ENOTDIR);
        return -1;
    }

    // Delegate to chdir, which verifies the path is a directory and
    // updates the CWD.  path_buf is already null-terminated by
    // get_fd_path.
    chdir(path_buf.as_ptr())
}

// isatty() is defined in ioctl.rs — it checks the fd table's HandleKind
// rather than hardcoding fd numbers, so it works for any Console fd.

/// Get the real user ID of the calling process.
///
/// Returns 0 (root) since we don't have multi-user support in
/// userspace yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getuid() -> UidT {
    0
}

/// Get the effective user ID of the calling process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn geteuid() -> UidT {
    0
}

/// Get the real group ID of the calling process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgid() -> GidT {
    0
}

/// Get the effective group ID of the calling process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getegid() -> GidT {
    0
}

/// Set the user ID of the calling process.
///
/// Stub: succeeds silently (single-user OS, always root).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setuid(_uid: UidT) -> i32 {
    0
}

/// Set the effective user ID of the calling process.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn seteuid(_uid: UidT) -> i32 {
    0
}

/// Set the group ID of the calling process.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setgid(_gid: GidT) -> i32 {
    0
}

/// Set the effective group ID of the calling process.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setegid(_gid: GidT) -> i32 {
    0
}

/// Set the real and effective user IDs.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setreuid(_ruid: UidT, _euid: UidT) -> i32 {
    0
}

/// Set the real and effective group IDs.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setregid(_rgid: GidT, _egid: GidT) -> i32 {
    0
}

/// Get the supplementary group IDs.
///
/// Returns 0 (no supplementary groups — only group 0).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgroups(_size: i32, _list: *mut GidT) -> i32 {
    0
}

/// Set the supplementary group IDs.
///
/// Stub: succeeds silently (single-user OS, no group enforcement).
/// Programs that drop privileges by calling `setgroups(0, NULL)` will
/// succeed without error.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setgroups(_size: usize, _list: *const GidT) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// Hostname storage
// ---------------------------------------------------------------------------

/// Maximum hostname length (references limits::HOST_NAME_MAX).
const HOST_NAME_MAX: usize = crate::limits::HOST_NAME_MAX as usize;

/// Hostname buffer (including null terminator space).
///
/// Initialized to "localhost" — can be changed via `sethostname()`.
/// SAFETY: single-process, no concurrency — direct access is safe.
static mut HOSTNAME_BUF: [u8; HOST_NAME_MAX + 1] = {
    let mut buf = [0u8; HOST_NAME_MAX + 1];
    // "localhost" = 9 bytes.
    buf[0] = b'l';
    buf[1] = b'o';
    buf[2] = b'c';
    buf[3] = b'a';
    buf[4] = b'l';
    buf[5] = b'h';
    buf[6] = b'o';
    buf[7] = b's';
    buf[8] = b't';
    buf
};

/// Length of the current hostname (excluding null terminator).
static mut HOSTNAME_LEN: usize = 9; // "localhost".len()

/// Domain name buffer (including null terminator space).
///
/// Initialized to "(none)" — can be changed via `setdomainname()`.
/// SAFETY: single-process, no concurrency — direct access is safe.
static mut DOMAIN_BUF: [u8; HOST_NAME_MAX + 1] = {
    let mut buf = [0u8; HOST_NAME_MAX + 1];
    // "(none)" = 6 bytes.
    buf[0] = b'(';
    buf[1] = b'n';
    buf[2] = b'o';
    buf[3] = b'n';
    buf[4] = b'e';
    buf[5] = b')';
    buf
};

/// Length of the current domain name (excluding null terminator).
static mut DOMAIN_LEN: usize = 6; // "(none)".len()

/// Get the hostname.
///
/// Copies the stored hostname into `name` (null-terminated).
/// Defaults to "localhost" until changed via `sethostname()`.
///
/// Returns 0 on success, -1 on error (ENAMETOOLONG if buffer too small).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gethostname(name: *mut u8, len: usize) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: single-address-space, no concurrent writes during read.
    // Use raw pointers to comply with Rust 2024 `static_mut_refs` rules.
    let (hostname_ptr, hlen) = unsafe {
        (&raw const HOSTNAME_BUF, HOSTNAME_LEN)
    };
    let needed = hlen.wrapping_add(1); // +null
    if len < needed {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }

    let mut idx: usize = 0;
    while idx < hlen {
        // SAFETY: idx < hlen <= HOST_NAME_MAX, HOSTNAME_BUF is HOST_NAME_MAX+1
        // bytes, and name buffer has at least `needed` bytes.
        unsafe {
            let byte = *hostname_ptr.cast::<u8>().add(idx);
            *name.add(idx) = byte;
        }
        idx = idx.wrapping_add(1);
    }
    // Null-terminate.
    // SAFETY: idx == hlen < len, so name.add(idx) is valid.
    unsafe { *name.add(idx) = 0; }
    0
}

/// Get the domain name of the host.
///
/// Copies the stored domain name into `name` (null-terminated).
/// Defaults to "(none)" until changed via `setdomainname()`.
///
/// Returns 0 on success, -1 on error (EINVAL if buffer too small).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getdomainname(name: *mut u8, len: usize) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: single-address-space, no concurrent writes during read.
    let (domain_ptr, dlen) = unsafe {
        (&raw const DOMAIN_BUF, DOMAIN_LEN)
    };
    let needed = dlen.wrapping_add(1); // +null
    if len < needed {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let mut idx: usize = 0;
    while idx < dlen {
        // SAFETY: idx < dlen <= HOST_NAME_MAX, DOMAIN_BUF is HOST_NAME_MAX+1
        // bytes, and name buffer has at least `needed` bytes.
        unsafe {
            let byte = *domain_ptr.cast::<u8>().add(idx);
            *name.add(idx) = byte;
        }
        idx = idx.wrapping_add(1);
    }
    // SAFETY: dlen < len (checked above).
    unsafe { *name.add(dlen) = 0; }
    0
}

/// Set the domain name of the host.
///
/// Stores `name[..len]` as the new domain name.  Subsequent calls to
/// `getdomainname()` will return the new value.
///
/// Returns 0 on success, -1 on error (EINVAL if too long, EFAULT if null).
///
/// Note: On a real multi-user system this would require `CAP_SYS_ADMIN`.
/// We're single-user so any process can set the domain name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setdomainname(name: *const u8, len: usize) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if len > HOST_NAME_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: single-address-space, no concurrent access.
    unsafe {
        let buf_ptr = (&raw mut DOMAIN_BUF).cast::<u8>();
        let mut idx = 0;
        while idx < len {
            *buf_ptr.add(idx) = *name.add(idx);
            idx = idx.wrapping_add(1);
        }
        *buf_ptr.add(len) = 0;
        DOMAIN_LEN = len;
    }
    0
}

/// Get the maximum number of open file descriptors.
///
/// Returns the size of the per-process file descriptor table.
/// This is a compatibility function; use `sysconf(_SC_OPEN_MAX)` in
/// new code.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getdtablesize() -> i32 {
    crate::fdtable::MAX_FDS as i32
}

/// Set an alarm timer.
///
/// Stub: returns 0 (no alarm support — signals not implemented).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn alarm(_seconds: u32) -> u32 {
    0
}

/// Set an alarm timer with microsecond granularity (deprecated BSD function).
///
/// Stub: returns 0 (no alarm support — signals not implemented).
/// `usecs` is the initial alarm delay in microseconds.
/// `interval` is the repeat interval in microseconds (0 = one-shot).
///
/// Returns the number of microseconds remaining from a previous alarm,
/// or 0 if none was set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ualarm(_usecs: u32, _interval: u32) -> u32 {
    0
}

/// Suspend until a signal is delivered.
///
/// Stub: sleeps for 1 second then returns -1/EINTR (no signals).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pause() -> i32 {
    let _ = syscall1(SYS_SLEEP, 1_000_000_000_u64);
    errno::set_errno(errno::EINTR);
    -1
}

/// Get configurable system variables.
///
/// Returns the value of the named system variable, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sysconf(name: i32) -> i64 {
    match name {
        _SC_PAGESIZE => 16384, // Our OS uses 16 KiB pages.
        _SC_NPROCESSORS_CONF | _SC_NPROCESSORS_ONLN => {
            // TODO: Query actual CPU count from kernel.
            1
        }
        _SC_OPEN_MAX => crate::fdtable::MAX_FDS as i64,
        _SC_LOGIN_NAME_MAX => i64::from(crate::limits::LOGIN_NAME_MAX),
        _SC_CLK_TCK => 100,            // 100 Hz timer tick (Linux default).
        _SC_ARG_MAX => i64::from(crate::limits::ARG_MAX),
        _SC_CHILD_MAX => 1024,         // Max child processes.
        _SC_IOV_MAX => i64::from(crate::limits::IOV_MAX),
        _SC_NGROUPS_MAX => i64::from(crate::limits::NGROUPS_MAX),
        _SC_VERSION | _SC_THREADS => 200_809,  // POSIX.1-2008 / threads supported (version).
        _SC_HOST_NAME_MAX => HOST_NAME_MAX as i64,
        _SC_LINE_MAX => i64::from(crate::limits::LINE_MAX),
        _SC_THREAD_STACK_MIN => 65536,  // 64 KiB minimum thread stack.
        _SC_PHYS_PAGES => 8192,         // ~128 MiB at 16 KiB pages (TODO: query kernel).
        _SC_AVPHYS_PAGES => 4096,       // ~64 MiB available (TODO: query kernel).
        _SC_GETPW_R_SIZE_MAX => 1024,   // Suggested passwd buffer size (glibc default).
        _SC_GETGR_R_SIZE_MAX => 1024,   // Suggested group buffer size.
        _SC_SYMLOOP_MAX => 40,          // Max symlink resolution depth (Linux default).
        _SC_STREAM_MAX => 16,           // Max stdio streams (our FILE_POOL size).
        _SC_TTY_NAME_MAX => i64::from(crate::limits::TTY_NAME_MAX),
        _SC_RE_DUP_MAX => 255,          // Max RE_DUP count (POSIX minimum 255).
        _SC_TZNAME_MAX => 6,            // Timezone name max (POSIX minimum 6).
        _SC_MQ_OPEN_MAX => i64::from(crate::limits::MQ_OPEN_MAX),
        _SC_MQ_PRIO_MAX => i64::from(crate::limits::MQ_PRIO_MAX),
        _SC_SEM_VALUE_MAX => i64::from(crate::limits::SEM_VALUE_MAX),
        _SC_TIMER_MAX => i64::from(crate::limits::TIMER_MAX),
        _SC_BC_BASE_MAX => i64::from(crate::limits::BC_BASE_MAX),
        _SC_BC_DIM_MAX => i64::from(crate::limits::BC_DIM_MAX),
        _SC_BC_SCALE_MAX => i64::from(crate::limits::BC_SCALE_MAX),
        _SC_BC_STRING_MAX => i64::from(crate::limits::BC_STRING_MAX),
        _SC_COLL_WEIGHTS_MAX => i64::from(crate::limits::COLL_WEIGHTS_MAX),
        _SC_EXPR_NEST_MAX => i64::from(crate::limits::EXPR_NEST_MAX),
        _SC_2_VERSION | _SC_2_C_BIND => 200_809,  // POSIX.1-2008 conformance.
        _SC_THREAD_DESTRUCTOR_ITERATIONS => i64::from(crate::limits::_POSIX_THREAD_DESTRUCTOR_ITERATIONS),
        _SC_THREAD_THREADS_MAX => 1024,   // Max threads (no hard kernel limit yet).
        _SC_THREAD_KEYS_MAX => i64::from(crate::limits::_POSIX_THREAD_KEYS_MAX),
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Get the page size of the system.
///
/// Our OS uses 16 KiB pages.  This is equivalent to
/// `sysconf(_SC_PAGESIZE)` but more convenient.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpagesize() -> i32 {
    16384
}

/// Adjust the program break (legacy heap interface).
///
/// Our OS uses mmap-based allocation exclusively — there is no
/// traditional brk/sbrk heap.  `sbrk(0)` returns a dummy address,
/// and `sbrk(n)` with `n != 0` fails with `ENOMEM`.
///
/// This stub exists for link compatibility with programs that
/// reference `sbrk`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sbrk(increment: isize) -> *mut u8 {
    if increment == 0 {
        // Return a non-NULL but meaningless address.
        // Some programs call sbrk(0) to find the "current break".
        return 0x1000_0000_usize as *mut u8;
    }
    // Cannot grow the heap — we use mmap.
    crate::errno::set_errno(crate::errno::ENOMEM);
    usize::MAX as *mut u8 // (void *)-1 signals failure
}

/// Set the program break (legacy heap interface).
///
/// Always fails — our OS uses mmap exclusively.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn brk(_addr: *mut u8) -> i32 {
    crate::errno::set_errno(crate::errno::ENOMEM);
    -1
}

// ---------------------------------------------------------------------------
// pathconf / fpathconf / confstr
// ---------------------------------------------------------------------------

/// POSIX `_PC_*` constants for `pathconf`/`fpathconf`.
#[allow(non_upper_case_globals)]
pub const _PC_LINK_MAX: i32 = 0;
#[allow(non_upper_case_globals)]
pub const _PC_MAX_CANON: i32 = 1;
#[allow(non_upper_case_globals)]
pub const _PC_MAX_INPUT: i32 = 2;
#[allow(non_upper_case_globals)]
pub const _PC_NAME_MAX: i32 = 3;
#[allow(non_upper_case_globals)]
pub const _PC_PATH_MAX: i32 = 4;
#[allow(non_upper_case_globals)]
pub const _PC_PIPE_BUF: i32 = 5;
#[allow(non_upper_case_globals)]
pub const _PC_CHOWN_RESTRICTED: i32 = 6;
#[allow(non_upper_case_globals)]
pub const _PC_NO_TRUNC: i32 = 7;
#[allow(non_upper_case_globals)]
pub const _PC_VDISABLE: i32 = 8;
#[allow(non_upper_case_globals)]
pub const _PC_FILESIZEBITS: i32 = 13;
#[allow(non_upper_case_globals)]
pub const _PC_SYMLINK_MAX: i32 = 19;

/// Get configurable pathname variables.
///
/// Returns the value of the named limit for `path`, or -1 if the
/// limit is indeterminate or the name is invalid.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pathconf(_path: *const u8, name: i32) -> i64 {
    // Return the same values regardless of path — we don't have
    // per-filesystem limits yet.
    match name {
        _PC_LINK_MAX => 127,                                      // Max hard links.
        _PC_MAX_CANON | _PC_MAX_INPUT => 255,                     // Terminal line limits.
        _PC_NAME_MAX => i64::from(crate::limits::NAME_MAX),       // Max filename length.
        _PC_PATH_MAX => PATH_MAX as i64,
        _PC_PIPE_BUF => i64::from(crate::limits::PIPE_BUF),      // Atomic pipe write size.
        _PC_CHOWN_RESTRICTED => 1,                                // chown restricted to root.
        _PC_NO_TRUNC => 1,                                        // Long names cause error.
        _PC_VDISABLE => 0,                                        // Characters can be disabled.
        _PC_FILESIZEBITS => 64,                                   // Max file size bits.
        _PC_SYMLINK_MAX => i64::from(crate::limits::SYMLINK_MAX), // Max symlink target length.
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Get configurable pathname variables for an open file.
///
/// Same as pathconf but takes a file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fpathconf(_fd: i32, name: i32) -> i64 {
    pathconf(core::ptr::null(), name)
}

/// `_CS_*` constants for `confstr`.
#[allow(non_upper_case_globals)]
pub const _CS_PATH: i32 = 0;

/// Get configuration-defined string values.
///
/// If `buf` is non-null and `len` > 0, copies the string into `buf`
/// (null-terminated).  Returns the total length needed (including
/// null), or 0 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn confstr(name: i32, buf: *mut u8, len: usize) -> usize {
    let value: &[u8] = if name == _CS_PATH {
        b"/bin:/usr/bin"
    } else {
        errno::set_errno(errno::EINVAL);
        return 0;
    };

    // Total size including null.
    let needed = value.len().wrapping_add(1);

    if !buf.is_null() && len > 0 {
        let copy_len = if value.len() < len { value.len() } else { len.wrapping_sub(1) };
        let mut i: usize = 0;
        while i < copy_len {
            if let Some(&b) = value.get(i) {
                // SAFETY: i < copy_len <= len, buf is valid for len bytes.
                unsafe { *buf.add(i) = b; }
            }
            i = i.wrapping_add(1);
        }
        // Null-terminate.
        unsafe { *buf.add(i) = 0; }
    }

    needed
}

// ---------------------------------------------------------------------------
// realpath
// ---------------------------------------------------------------------------

/// Canonicalize a pathname.
///
/// Resolves `.`, `..`, redundant `/`, and relative paths against the
/// CWD to produce a normalized absolute path.  Verifies the target
/// exists via `SYS_FS_STAT`.
///
/// `resolved_path` must point to a buffer of at least `PATH_MAX`
/// bytes.  If null, returns null with `EINVAL` (POSIX allows malloc
/// fallback, but we are `no_std`).
///
/// **Limitation**: symlinks are not followed.  The returned path has
/// `.`/`..` resolved and is absolute, but intermediate symlink
/// components are not dereferenced.  This matches `realpath -s`
/// semantics on some systems.
///
/// Returns `resolved_path` on success, null on error with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn realpath(path: *const u8, resolved_path: *mut u8) -> *mut u8 {
    if path.is_null() {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }
    if resolved_path.is_null() {
        // POSIX says we may malloc; we can't (no_std).
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    }

    // Resolve relative path against CWD and normalize.
    let mut resolved = [0u8; PATH_MAX];
    let Some(resolved_len) = (unsafe { resolve_path(path, &mut resolved) }) else {
        // POSIX: empty path → ENOENT; too-long → ENAMETOOLONG.
        // SAFETY: path is non-null (checked above) and a valid C string.
        if unsafe { *path } == 0 {
            errno::set_errno(errno::ENOENT);
        } else {
            errno::set_errno(errno::ENAMETOOLONG);
        }
        return core::ptr::null_mut();
    };

    // Verify the path exists.
    let mut stat_buf = core::mem::MaybeUninit::<crate::stat::Stat>::zeroed();
    let ret = syscall3(
        SYS_FS_STAT,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        stat_buf.as_mut_ptr() as u64,
    );

    if ret < 0 {
        let _ = errno::translate(ret);
        return core::ptr::null_mut();
    }

    // Copy to caller's buffer and null-terminate.
    // SAFETY: resolved_path is valid for PATH_MAX bytes (caller contract).
    unsafe {
        for i in 0..resolved_len {
            if let Some(&b) = resolved.get(i) {
                *resolved_path.add(i) = b;
            }
        }
        *resolved_path.add(resolved_len) = 0;
    }

    resolved_path
}

// ---------------------------------------------------------------------------
// sync / sethostname / chroot
// ---------------------------------------------------------------------------

/// Commit all filesystem caches to stable storage.
///
/// Stub: no-op.  Our filesystem doesn't have a write-back cache yet,
/// so there is nothing to flush.  Always succeeds (void return per
/// POSIX).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sync() {
    // No-op: filesystem writes are synchronous.
}

/// Set the system hostname.
///
/// Stores the hostname in a process-local static buffer.  Retrieved
/// via `gethostname()`.  Maximum length is 255 bytes (HOST_NAME_MAX).
///
/// Returns 0 on success, -1 on error (EINVAL if too long, EFAULT if null).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sethostname(name: *const u8, len: usize) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if len > HOST_NAME_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: single-address-space, no concurrent access.
    // Use raw pointers to comply with Rust 2024 `static_mut_refs` rules.
    unsafe {
        let buf_ptr = (&raw mut HOSTNAME_BUF).cast::<u8>();
        let mut idx = 0;
        while idx < len {
            *buf_ptr.add(idx) = *name.add(idx);
            idx = idx.wrapping_add(1);
        }
        // Null-terminate the stored hostname.
        *buf_ptr.add(len) = 0;
        HOSTNAME_LEN = len;
    }
    0
}

/// Change the root directory.
///
/// Stub: returns -1 with `ENOSYS`.  Filesystem namespaces are not
/// yet implemented.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chroot(_path: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// daemon
// ---------------------------------------------------------------------------

/// Detach from the controlling terminal and run in the background.
///
/// If `nochdir` is 0, changes the working directory to `/`.
/// If `noclose` is 0, redirects stdin/stdout/stderr to `/dev/null`
/// (stubbed — we don't have `/dev/null`, so we just close them).
///
/// Our OS doesn't have `fork()`, so this is a best-effort stub that
/// performs the CWD change and fd redirection but cannot actually
/// create a background process.  Programs that call `daemon()` will
/// continue running in the foreground.
///
/// Returns 0 on success, -1 on error with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn daemon(nochdir: i32, noclose: i32) -> i32 {
    // Change CWD to root unless suppressed.
    if nochdir == 0 {
        let root = b"/\0";
        let ret = chdir(root.as_ptr());
        if ret < 0 {
            return -1;
        }
    }

    // Close standard fds unless suppressed.
    // A real daemon would reopen them to /dev/null, but we don't have
    // /dev/null yet.  Closing them prevents accidental terminal output.
    if noclose == 0 {
        crate::file::close(STDIN_FILENO);
        crate::file::close(STDOUT_FILENO);
        crate::file::close(STDERR_FILENO);
    }

    // Cannot fork — we stay in the same process.  Call setsid() to
    // create a new session (best effort at detaching).
    let _ = crate::process::setsid();

    0
}

// ---------------------------------------------------------------------------
// getloadavg
// ---------------------------------------------------------------------------

/// Get system load averages.
///
/// Fills `loadavg` with up to `nelem` load average values (1-min,
/// 5-min, 15-min).  Returns the number of samples stored, or -1 on
/// error.
///
/// Stub: returns synthetic idle-system values (0.0) since our OS
/// doesn't track load averages yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32 {
    if loadavg.is_null() || nelem <= 0 {
        return -1;
    }

    // Clamp to 3 (POSIX defines at most 3 load averages).
    let count = if nelem > 3 { 3 } else { nelem };

    let mut i: i32 = 0;
    while i < count {
        // SAFETY: loadavg is valid for at least nelem elements (caller
        // contract), and i < count <= nelem.
        unsafe {
            *loadavg.add(i as usize) = 0.0;
        }
        i = i.wrapping_add(1);
    }

    count
}

// ---------------------------------------------------------------------------
// getrandom / getentropy
// ---------------------------------------------------------------------------

/// Flags for `getrandom`.
pub const GRND_NONBLOCK: u32 = 1;
/// Use the random source (not urandom).
pub const GRND_RANDOM: u32 = 2;

/// Fill a buffer with random bytes.
///
/// Uses `rdrand` x86_64 instruction where available.  Falls back to
/// a simple LCG seeded from the monotonic clock if RDRAND fails.
///
/// Returns the number of bytes filled, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrandom(buf: *mut u8, buflen: usize, _flags: u32) -> isize {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Guard against buflen > isize::MAX to avoid returning a negative
    // value that callers would interpret as an error.
    if buflen > isize::MAX as usize {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    fill_random(buf, buflen);
    buflen as isize
}

/// Fill a buffer with random bytes (simplified API).
///
/// Like `getrandom` but with no flags and returns 0/errno.
/// Maximum 256 bytes per call (POSIX requirement).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getentropy(buf: *mut u8, buflen: usize) -> i32 {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if buflen > 256 {
        errno::set_errno(errno::EIO);
        return -1;
    }

    fill_random(buf, buflen);
    0
}

/// Fill a buffer with pseudo-random bytes.
///
/// Tries RDRAND first (hardware RNG), falls back to an LCG seeded from
/// the monotonic clock.  Not cryptographically strong — suitable for
/// seeding userspace PRNGs, temp file names, etc.
fn fill_random(buf: *mut u8, len: usize) {
    // Try RDRAND first.
    let mut seed: u64 = 0;
    let rdrand_ok: bool;

    #[cfg(target_arch = "x86_64")]
    {
        let ok: u8;
        // SAFETY: rdrand is safe to execute; it simply reads hardware RNG.
        unsafe {
            core::arch::asm!(
                "rdrand {val}",
                "setc {ok}",
                val = out(reg) seed,
                ok = out(reg_byte) ok,
                options(nostack, nomem),
            );
        }
        rdrand_ok = ok != 0;
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        rdrand_ok = false;
    }

    if !rdrand_ok {
        // Fallback: seed from monotonic clock.
        let ns = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        seed = ns;
    }

    // Use a simple LCG to fill the buffer.  XOR with RDRAND output
    // if available for better entropy distribution.
    let mut state = seed;
    let mut i: usize = 0;
    while i < len {
        // LCG step: state = state * 6364136223846793005 + 1442695040888963407
        state = state
            .wrapping_mul(0x5851_F42D_4C95_7F2D)
            .wrapping_add(0x1405_7B7E_F767_814F);

        // Extract byte from upper bits (better quality).
        let byte = (state >> 56) as u8;
        // SAFETY: i < len, buf is valid for len bytes.
        unsafe { *buf.add(i) = byte; }
        i = i.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// fdatasync / syncfs
// ---------------------------------------------------------------------------

/// Flush data (not metadata) for an open file descriptor.
///
/// Stub: delegates to `fsync` (we don't distinguish data-only sync).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdatasync(fd: Fd) -> i32 {
    // Our fsync already just returns 0 (filesystem writes are sync).
    crate::file::fsync(fd)
}

/// Synchronize all data for the filesystem containing `fd`.
///
/// Stub: no-op (same as `sync` — our writes are synchronous).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn syncfs(_fd: Fd) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// abort
// ---------------------------------------------------------------------------

/// Write a message to standard error and abort.
///
/// Not exactly POSIX, but commonly needed by C runtime init code.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn abort() -> ! {
    // Write "Aborted\n" to stderr (console).
    let msg = b"Aborted\n";
    let _ = syscall2(SYS_CONSOLE_WRITE, msg.as_ptr() as u64, msg.len() as u64);
    #[allow(clippy::used_underscore_items)] // _exit is the POSIX name.
    crate::process::_exit(134); // 128 + SIGABRT(6)
}

// ---------------------------------------------------------------------------
// prctl — process control (Linux)
// ---------------------------------------------------------------------------

/// prctl options.
pub const PR_SET_NAME: i32 = 15;
/// Get process name.
pub const PR_GET_NAME: i32 = 16;
/// Set "no new privileges" flag.
pub const PR_SET_NO_NEW_PRIVS: i32 = 38;
/// Get "no new privileges" flag.
pub const PR_GET_NO_NEW_PRIVS: i32 = 39;
/// Set seccomp mode.
pub const PR_SET_SECCOMP: i32 = 22;
/// Get seccomp mode.
pub const PR_GET_SECCOMP: i32 = 21;

/// Process control operations (Linux).
///
/// Stub: `PR_SET_NAME` and `PR_SET_NO_NEW_PRIVS` succeed silently.
/// Other operations return -1 with EINVAL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn prctl(option: i32, arg2: u64, _arg3: u64, _arg4: u64, _arg5: u64) -> i32 {
    match option {
        PR_SET_NAME | PR_SET_NO_NEW_PRIVS | PR_GET_NO_NEW_PRIVS => 0,
        PR_GET_NAME => {
            // Would need to write a name into arg2 as a buffer.
            // Return empty name for now.
            if arg2 != 0 {
                // SAFETY: Caller provides valid buffer per prctl contract.
                unsafe { *(arg2 as *mut u8) = 0; }
            }
            0
        }
        _ => {
            crate::errno::set_errno(crate::errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Linux misc: setresuid/setresgid/getresuid/getresgid
// ---------------------------------------------------------------------------

/// Set real, effective, and saved set-user-ID.
///
/// Stub: succeeds silently (single-user system).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setresuid(_ruid: UidT, _euid: UidT, _suid: UidT) -> i32 {
    0
}

/// Set real, effective, and saved set-group-ID.
///
/// Stub: succeeds silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setresgid(_rgid: GidT, _egid: GidT, _sgid: GidT) -> i32 {
    0
}

/// Get real, effective, and saved set-user-ID.
///
/// Stub: returns 0 (root) for all three.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getresuid(ruid: *mut UidT, euid: *mut UidT, suid: *mut UidT) -> i32 {
    if ruid.is_null() || euid.is_null() || suid.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: All pointers verified non-null.
    unsafe {
        *ruid = 0;
        *euid = 0;
        *suid = 0;
    }
    0
}

/// Get real, effective, and saved set-group-ID.
///
/// Stub: returns 0 (root) for all three.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getresgid(rgid: *mut GidT, egid: *mut GidT, sgid: *mut GidT) -> i32 {
    if rgid.is_null() || egid.is_null() || sgid.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: All pointers verified non-null.
    unsafe {
        *rgid = 0;
        *egid = 0;
        *sgid = 0;
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // normalize_path — pure function, exhaustively testable
    // ------------------------------------------------------------------

    #[test]
    fn norm_root() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/", &mut out).unwrap();
        assert_eq!(&out[..len], b"/");
    }

    #[test]
    fn norm_simple_path() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/foo", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo");
    }

    #[test]
    fn norm_trailing_slash() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/foo/", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo");
    }

    #[test]
    fn norm_double_slash() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"//foo", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo");
    }

    #[test]
    fn norm_many_slashes() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"///foo///bar///", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo/bar");
    }

    #[test]
    fn norm_dot() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/foo/./bar", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo/bar");
    }

    #[test]
    fn norm_dot_at_end() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/foo/.", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo");
    }

    #[test]
    fn norm_dotdot() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/foo/bar/..", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo");
    }

    #[test]
    fn norm_dotdot_to_root() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/foo/..", &mut out).unwrap();
        assert_eq!(&out[..len], b"/");
    }

    #[test]
    fn norm_dotdot_beyond_root() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/..", &mut out).unwrap();
        assert_eq!(&out[..len], b"/");
    }

    #[test]
    fn norm_multiple_dotdot_beyond_root() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/../../..", &mut out).unwrap();
        assert_eq!(&out[..len], b"/");
    }

    #[test]
    fn norm_complex_mixed() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/a/b/../c/./d/../e", &mut out).unwrap();
        assert_eq!(&out[..len], b"/a/c/e");
    }

    #[test]
    fn norm_multi_dotdot() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/a/b/c/../../d", &mut out).unwrap();
        assert_eq!(&out[..len], b"/a/d");
    }

    #[test]
    fn norm_only_slashes() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"////", &mut out).unwrap();
        assert_eq!(&out[..len], b"/");
    }

    #[test]
    fn norm_rejects_relative() {
        let mut out = [0u8; PATH_MAX];
        assert!(normalize_path(b"foo/bar", &mut out).is_none());
    }

    #[test]
    fn norm_rejects_empty() {
        let mut out = [0u8; PATH_MAX];
        assert!(normalize_path(b"", &mut out).is_none());
    }

    #[test]
    fn norm_deep_nesting() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/a/b/c/d/e/f/g", &mut out).unwrap();
        assert_eq!(&out[..len], b"/a/b/c/d/e/f/g");
    }

    #[test]
    fn norm_dotdot_preserves_sibling() {
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/home/user/../other/file.txt", &mut out).unwrap();
        assert_eq!(&out[..len], b"/home/other/file.txt");
    }

    #[test]
    fn norm_dot_files_preserved() {
        // ".hidden" is a regular component, not a "." directive.
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/.hidden/..config", &mut out).unwrap();
        assert_eq!(&out[..len], b"/.hidden/..config");
    }

    #[test]
    fn norm_three_dots_preserved() {
        // "..." is a regular component, not ".." or ".".
        let mut out = [0u8; PATH_MAX];
        let len = normalize_path(b"/foo/.../bar", &mut out).unwrap();
        assert_eq!(&out[..len], b"/foo/.../bar");
    }

    // ------------------------------------------------------------------
    // PATH_MAX constant
    // ------------------------------------------------------------------

    #[test]
    fn path_max_is_4096() {
        assert_eq!(PATH_MAX, 4096);
    }

    // ------------------------------------------------------------------
    // sysconf — system configuration variables
    // ------------------------------------------------------------------

    #[test]
    fn test_sysconf_pagesize() {
        assert_eq!(sysconf(_SC_PAGESIZE), 16384);
        assert_eq!(sysconf(_SC_PAGE_SIZE), 16384);
    }

    #[test]
    fn test_sysconf_open_max() {
        assert_eq!(sysconf(_SC_OPEN_MAX), crate::fdtable::MAX_FDS as i64);
    }

    #[test]
    fn test_sysconf_clk_tck() {
        assert_eq!(sysconf(_SC_CLK_TCK), 100);
    }

    #[test]
    fn test_sysconf_arg_max() {
        assert_eq!(sysconf(_SC_ARG_MAX), 131_072);
    }

    #[test]
    fn test_sysconf_version() {
        assert_eq!(sysconf(_SC_VERSION), 200_809);
    }

    #[test]
    fn test_sysconf_phys_pages() {
        assert!(sysconf(_SC_PHYS_PAGES) > 0);
    }

    #[test]
    fn test_sysconf_getpw_r_size_max() {
        let val = sysconf(_SC_GETPW_R_SIZE_MAX);
        assert!(val > 0, "GETPW_R_SIZE_MAX should be positive");
    }

    #[test]
    fn test_sysconf_getgr_r_size_max() {
        let val = sysconf(_SC_GETGR_R_SIZE_MAX);
        assert!(val > 0, "GETGR_R_SIZE_MAX should be positive");
    }

    #[test]
    fn test_sysconf_symloop_max() {
        let val = sysconf(_SC_SYMLOOP_MAX);
        assert!(val >= 8, "SYMLOOP_MAX should be at least 8");
    }

    #[test]
    fn test_sysconf_stream_max() {
        let val = sysconf(_SC_STREAM_MAX);
        assert!(val > 0, "STREAM_MAX should be positive");
    }

    #[test]
    fn test_sysconf_tty_name_max() {
        assert_eq!(
            sysconf(_SC_TTY_NAME_MAX),
            i64::from(crate::limits::TTY_NAME_MAX),
        );
    }

    #[test]
    fn test_sysconf_re_dup_max() {
        let val = sysconf(_SC_RE_DUP_MAX);
        assert!(val >= 255, "RE_DUP_MAX should be at least POSIX minimum 255");
    }

    #[test]
    fn test_sysconf_unknown_returns_negative() {
        assert_eq!(sysconf(-999), -1);
    }

    #[test]
    fn test_sysconf_new_constants() {
        // Verify all newly added _SC_* constants return positive values.
        let new_names = [
            _SC_TZNAME_MAX, _SC_MQ_OPEN_MAX, _SC_MQ_PRIO_MAX,
            _SC_SEM_VALUE_MAX, _SC_TIMER_MAX,
            _SC_BC_BASE_MAX, _SC_BC_DIM_MAX, _SC_BC_SCALE_MAX,
            _SC_BC_STRING_MAX, _SC_COLL_WEIGHTS_MAX, _SC_EXPR_NEST_MAX,
            _SC_2_VERSION, _SC_2_C_BIND,
            _SC_THREAD_DESTRUCTOR_ITERATIONS, _SC_THREAD_THREADS_MAX,
            _SC_THREAD_KEYS_MAX,
        ];
        for &name in &new_names {
            let val = sysconf(name);
            assert!(
                val > 0,
                "sysconf({name}) should return a positive value, got {val}"
            );
        }
    }

    #[test]
    fn test_sysconf_mq_limits() {
        assert_eq!(sysconf(_SC_MQ_OPEN_MAX), i64::from(crate::limits::MQ_OPEN_MAX));
        assert_eq!(sysconf(_SC_MQ_PRIO_MAX), i64::from(crate::limits::MQ_PRIO_MAX));
    }

    #[test]
    fn test_sysconf_bc_limits() {
        assert_eq!(sysconf(_SC_BC_BASE_MAX), 99);
        assert_eq!(sysconf(_SC_BC_DIM_MAX), 2048);
        assert_eq!(sysconf(_SC_BC_SCALE_MAX), 99);
        assert_eq!(sysconf(_SC_BC_STRING_MAX), 1000);
    }

    #[test]
    fn test_sc_constants_unique() {
        // All _SC_* constants must be distinct (except aliases).
        let vals: &[i32] = &[
            _SC_ARG_MAX, _SC_CHILD_MAX, _SC_CLK_TCK, _SC_NGROUPS_MAX,
            _SC_OPEN_MAX, _SC_STREAM_MAX, _SC_TZNAME_MAX,
            _SC_PAGESIZE, _SC_VERSION, _SC_THREADS,
            _SC_HOST_NAME_MAX, _SC_LOGIN_NAME_MAX, _SC_LINE_MAX,
            _SC_THREAD_STACK_MIN, _SC_PHYS_PAGES, _SC_AVPHYS_PAGES,
            _SC_IOV_MAX, _SC_GETPW_R_SIZE_MAX, _SC_GETGR_R_SIZE_MAX,
            _SC_SYMLOOP_MAX, _SC_TTY_NAME_MAX, _SC_RE_DUP_MAX,
            _SC_NPROCESSORS_CONF, _SC_NPROCESSORS_ONLN,
            _SC_MQ_OPEN_MAX, _SC_MQ_PRIO_MAX, _SC_SEM_VALUE_MAX,
            _SC_TIMER_MAX, _SC_BC_BASE_MAX, _SC_BC_DIM_MAX,
            _SC_BC_SCALE_MAX, _SC_BC_STRING_MAX, _SC_COLL_WEIGHTS_MAX,
            _SC_EXPR_NEST_MAX, _SC_2_VERSION, _SC_2_C_BIND,
            _SC_THREAD_DESTRUCTOR_ITERATIONS, _SC_THREAD_THREADS_MAX,
            _SC_THREAD_KEYS_MAX,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(
                    vals[i], vals[j],
                    "_SC constants at indices {i} and {j} must be distinct (both = {})",
                    vals[i]
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // getpagesize / getdtablesize
    // ------------------------------------------------------------------

    #[test]
    fn test_getpagesize() {
        assert_eq!(getpagesize(), 16384);
    }

    #[test]
    fn test_getdtablesize() {
        assert_eq!(getdtablesize(), crate::fdtable::MAX_FDS as i32);
    }

    #[test]
    fn test_getdtablesize_matches_sysconf() {
        // These must agree — both derive from fdtable::MAX_FDS.
        assert_eq!(
            getdtablesize() as i64,
            sysconf(_SC_OPEN_MAX),
            "getdtablesize() and sysconf(_SC_OPEN_MAX) must match"
        );
    }

    // ------------------------------------------------------------------
    // pathconf / fpathconf
    // ------------------------------------------------------------------

    #[test]
    fn test_pathconf_path_max() {
        assert_eq!(pathconf(core::ptr::null(), _PC_PATH_MAX), PATH_MAX as i64);
    }

    #[test]
    fn test_pathconf_name_max() {
        assert_eq!(
            pathconf(core::ptr::null(), _PC_NAME_MAX),
            i64::from(crate::limits::NAME_MAX),
        );
    }

    #[test]
    fn test_pathconf_pipe_buf() {
        assert_eq!(
            pathconf(core::ptr::null(), _PC_PIPE_BUF),
            i64::from(crate::limits::PIPE_BUF),
        );
    }

    #[test]
    fn test_pathconf_link_max() {
        assert_eq!(pathconf(core::ptr::null(), _PC_LINK_MAX), 127);
    }

    #[test]
    fn test_pathconf_unknown_returns_negative() {
        assert_eq!(pathconf(core::ptr::null(), -999), -1);
    }

    #[test]
    fn test_fpathconf_delegates_to_pathconf() {
        // fpathconf should return the same values as pathconf.
        assert_eq!(fpathconf(0, _PC_PATH_MAX), pathconf(core::ptr::null(), _PC_PATH_MAX));
        assert_eq!(fpathconf(0, _PC_NAME_MAX), pathconf(core::ptr::null(), _PC_NAME_MAX));
    }

    #[test]
    fn test_pathconf_filesizebits() {
        assert_eq!(pathconf(core::ptr::null(), _PC_FILESIZEBITS), 64);
    }

    #[test]
    fn test_pathconf_symlink_max() {
        assert_eq!(
            pathconf(core::ptr::null(), _PC_SYMLINK_MAX),
            i64::from(crate::limits::SYMLINK_MAX)
        );
    }

    // ------------------------------------------------------------------
    // confstr
    // ------------------------------------------------------------------

    #[test]
    fn test_confstr_cs_path_size() {
        // With null buf, returns needed size.
        let needed = confstr(_CS_PATH, core::ptr::null_mut(), 0);
        // "/bin:/usr/bin" (13 chars) + '\0' = 14 bytes.
        assert_eq!(needed, 14);
    }

    #[test]
    fn test_confstr_cs_path_copies() {
        let mut buf = [0xFFu8; 64];
        let needed = confstr(_CS_PATH, buf.as_mut_ptr(), buf.len());
        assert_eq!(needed, 14);
        assert_eq!(&buf[..14], b"/bin:/usr/bin\0");
    }

    #[test]
    fn test_confstr_cs_path_truncation() {
        let mut buf = [0xFFu8; 6]; // Smaller than needed.
        let needed = confstr(_CS_PATH, buf.as_mut_ptr(), buf.len());
        assert_eq!(needed, 14); // Still returns full needed size.
        // Should have written 5 chars + null.
        assert_eq!(&buf[..6], b"/bin:\0");
    }

    #[test]
    fn test_confstr_unknown_returns_zero() {
        assert_eq!(confstr(-999, core::ptr::null_mut(), 0), 0);
    }

    // ------------------------------------------------------------------
    // sbrk / brk
    // ------------------------------------------------------------------

    #[test]
    fn test_sbrk_zero_returns_address() {
        let ptr = sbrk(0);
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_sbrk_nonzero_fails() {
        let ptr = sbrk(4096);
        assert_eq!(ptr, usize::MAX as *mut u8); // (void *)-1
    }

    #[test]
    fn test_brk_always_fails() {
        assert_eq!(brk(core::ptr::null_mut()), -1);
    }

    // ------------------------------------------------------------------
    // uid/gid stubs (single-user → always 0/root)
    // ------------------------------------------------------------------

    #[test]
    fn test_getuid_root() {
        assert_eq!(getuid(), 0);
    }

    #[test]
    fn test_geteuid_root() {
        assert_eq!(geteuid(), 0);
    }

    #[test]
    fn test_getgid_root() {
        assert_eq!(getgid(), 0);
    }

    #[test]
    fn test_getegid_root() {
        assert_eq!(getegid(), 0);
    }

    #[test]
    fn test_setuid_succeeds() {
        assert_eq!(setuid(0), 0);
    }

    #[test]
    fn test_setgid_succeeds() {
        assert_eq!(setgid(0), 0);
    }

    // ------------------------------------------------------------------
    // getresuid / getresgid
    // ------------------------------------------------------------------

    #[test]
    fn test_getresuid_fills_zeros() {
        let mut ruid: UidT = 99;
        let mut euid: UidT = 99;
        let mut suid: UidT = 99;
        let ret = getresuid(&raw mut ruid, &raw mut euid, &raw mut suid);
        assert_eq!(ret, 0);
        assert_eq!(ruid, 0);
        assert_eq!(euid, 0);
        assert_eq!(suid, 0);
    }

    #[test]
    fn test_getresuid_null_fails() {
        assert_eq!(getresuid(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_getresgid_fills_zeros() {
        let mut rgid: GidT = 99;
        let mut egid: GidT = 99;
        let mut sgid: GidT = 99;
        let ret = getresgid(&raw mut rgid, &raw mut egid, &raw mut sgid);
        assert_eq!(ret, 0);
        assert_eq!(rgid, 0);
        assert_eq!(egid, 0);
        assert_eq!(sgid, 0);
    }

    #[test]
    fn test_getresgid_null_fails() {
        assert_eq!(getresgid(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_setresuid_succeeds() {
        assert_eq!(setresuid(0, 0, 0), 0);
    }

    #[test]
    fn test_setresgid_succeeds() {
        assert_eq!(setresgid(0, 0, 0), 0);
    }

    // ------------------------------------------------------------------
    // prctl stubs
    // ------------------------------------------------------------------

    #[test]
    fn test_prctl_set_name_succeeds() {
        assert_eq!(prctl(PR_SET_NAME, 0, 0, 0, 0), 0);
    }

    #[test]
    fn test_prctl_set_no_new_privs_succeeds() {
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
    }

    #[test]
    fn test_prctl_get_name_writes_null() {
        let mut buf = [0xFFu8; 16];
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_prctl_unknown_fails() {
        assert_eq!(prctl(-999, 0, 0, 0, 0), -1);
    }

    // ------------------------------------------------------------------
    // alarm / getgroups stubs
    // ------------------------------------------------------------------

    #[test]
    fn test_alarm_returns_zero() {
        assert_eq!(alarm(0), 0);
        assert_eq!(alarm(10), 0);
    }

    #[test]
    fn test_getgroups_zero_size_returns_zero() {
        // POSIX: getgroups(0, NULL) returns number of supplementary groups.
        // Our stub returns 0.
        assert_eq!(getgroups(0, core::ptr::null_mut()), 0);
    }

    // ------------------------------------------------------------------
    // syncfs / fdatasync stubs
    // ------------------------------------------------------------------

    #[test]
    fn test_syncfs_succeeds() {
        assert_eq!(syncfs(0), 0);
    }

    // ------------------------------------------------------------------
    // _SC_ constant values
    // ------------------------------------------------------------------

    #[test]
    fn test_sc_constant_values() {
        assert_eq!(_SC_PAGESIZE, 30);
        assert_eq!(_SC_PAGE_SIZE, _SC_PAGESIZE);
        assert_eq!(_SC_OPEN_MAX, 4);
        assert_eq!(_SC_CLK_TCK, 2);
        assert_eq!(_SC_ARG_MAX, 0);
        assert_eq!(_SC_NPROCESSORS_CONF, 83);
        assert_eq!(_SC_NPROCESSORS_ONLN, 84);
        assert_eq!(_SC_GETPW_R_SIZE_MAX, 70);
        assert_eq!(_SC_GETGR_R_SIZE_MAX, 69);
        assert_eq!(_SC_SYMLOOP_MAX, 173);
        assert_eq!(_SC_STREAM_MAX, 5);
        assert_eq!(_SC_TTY_NAME_MAX, 72);
        assert_eq!(_SC_RE_DUP_MAX, 44);
    }

    // ------------------------------------------------------------------
    // _PC_ constant values
    // ------------------------------------------------------------------

    #[test]
    fn test_pc_constant_values() {
        assert_eq!(_PC_LINK_MAX, 0);
        assert_eq!(_PC_MAX_CANON, 1);
        assert_eq!(_PC_MAX_INPUT, 2);
        assert_eq!(_PC_NAME_MAX, 3);
        assert_eq!(_PC_PATH_MAX, 4);
        assert_eq!(_PC_PIPE_BUF, 5);
        assert_eq!(_PC_CHOWN_RESTRICTED, 6);
        assert_eq!(_PC_NO_TRUNC, 7);
        assert_eq!(_PC_VDISABLE, 8);
    }

    // ------------------------------------------------------------------
    // gethostname / sethostname
    // ------------------------------------------------------------------

    #[test]
    fn test_gethostname_default() {
        let mut buf = [0u8; 256];
        assert_eq!(gethostname(buf.as_mut_ptr(), buf.len()), 0);
        // Default is "localhost" (unless changed by a prior test).
        // Just verify it returns a non-empty string.
        assert_ne!(buf[0], 0, "hostname should be non-empty");
    }

    #[test]
    fn test_gethostname_null() {
        assert_eq!(gethostname(core::ptr::null_mut(), 10), -1);
    }

    #[test]
    fn test_sethostname_roundtrip() {
        // Save the original hostname.
        let mut orig = [0u8; 256];
        gethostname(orig.as_mut_ptr(), orig.len());
        let orig_len = unsafe { crate::string::strlen(orig.as_ptr()) };

        // Set a new hostname.
        let new_name = b"test-host";
        assert_eq!(sethostname(new_name.as_ptr(), new_name.len()), 0);

        // Verify it was set.
        let mut buf = [0u8; 256];
        assert_eq!(gethostname(buf.as_mut_ptr(), buf.len()), 0);
        assert_eq!(&buf[..new_name.len()], new_name);

        // Restore the original.
        sethostname(orig.as_ptr(), orig_len);
    }

    #[test]
    fn test_sethostname_null() {
        assert_eq!(sethostname(core::ptr::null(), 5), -1);
    }

    #[test]
    fn test_sethostname_too_long() {
        let long_name = [b'a'; 256]; // HOST_NAME_MAX = 255
        assert_eq!(sethostname(long_name.as_ptr(), 256), -1);
    }

    // ------------------------------------------------------------------
    // getdomainname / setdomainname
    // ------------------------------------------------------------------

    #[test]
    fn test_getdomainname_default() {
        let mut buf = [0u8; 256];
        assert_eq!(getdomainname(buf.as_mut_ptr(), buf.len()), 0);
        // Default is "(none)".
        assert_eq!(&buf[..6], b"(none)");
    }

    #[test]
    fn test_getdomainname_null() {
        assert_eq!(getdomainname(core::ptr::null_mut(), 10), -1);
    }

    #[test]
    fn test_setdomainname_roundtrip() {
        // Save the original.
        let mut orig = [0u8; 256];
        getdomainname(orig.as_mut_ptr(), orig.len());
        let orig_len = unsafe { crate::string::strlen(orig.as_ptr()) };

        // Set a new domain.
        let new_domain = b"example.com";
        assert_eq!(setdomainname(new_domain.as_ptr(), new_domain.len()), 0);

        // Verify it was set.
        let mut buf = [0u8; 256];
        assert_eq!(getdomainname(buf.as_mut_ptr(), buf.len()), 0);
        assert_eq!(&buf[..new_domain.len()], new_domain);

        // Restore the original.
        setdomainname(orig.as_ptr(), orig_len);
    }

    #[test]
    fn test_setdomainname_null() {
        assert_eq!(setdomainname(core::ptr::null(), 5), -1);
    }

    #[test]
    fn test_setdomainname_too_long() {
        let long_name = [b'a'; 256]; // HOST_NAME_MAX = 255
        assert_eq!(setdomainname(long_name.as_ptr(), 256), -1);
    }

    #[test]
    fn test_getdomainname_buffer_too_small() {
        // "(none)" + null = 7 bytes. Buffer of 5 is too small.
        // Reset to known state first.
        let default = b"(none)";
        setdomainname(default.as_ptr(), default.len());

        let mut buf = [0u8; 5];
        assert_eq!(getdomainname(buf.as_mut_ptr(), buf.len()), -1);
    }

    // ------------------------------------------------------------------
    // POSIX feature-test constants
    // ------------------------------------------------------------------

    #[test]
    fn test_posix_version_constant() {
        assert_eq!(_POSIX_VERSION, 200_809);
    }

    #[test]
    fn test_xopen_version_constant() {
        assert_eq!(_XOPEN_VERSION, 700);
    }

    #[test]
    fn test_posix_feature_macros_are_posix_2008() {
        // All feature macros that indicate version should be 200809.
        assert_eq!(_POSIX_THREADS, 200_809);
        assert_eq!(_POSIX_MAPPED_FILES, 200_809);
        assert_eq!(_POSIX_MEMORY_PROTECTION, 200_809);
        assert_eq!(_POSIX_FSYNC, 200_809);
        assert_eq!(_POSIX_TIMERS, 200_809);
        assert_eq!(_POSIX_MONOTONIC_CLOCK, 200_809);
        assert_eq!(_POSIX_CLOCK_SELECTION, 200_809);
    }

    #[test]
    fn test_posix_boolean_features() {
        assert_eq!(_POSIX_SAVED_IDS, 1);
        assert_eq!(_POSIX_JOB_CONTROL, 1);
    }
}
