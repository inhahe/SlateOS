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

    // Verify the target exists and is a directory.  SYS_FS_STAT writes a
    // 16-byte FsStatResult, not a struct stat, so translate it.
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let ret = syscall3(
        SYS_FS_STAT,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        raw.as_mut_ptr() as u64,
    );

    if ret < 0 {
        return errno::translate(ret) as i32;
    }

    let mut sb = crate::stat::Stat::zeroed();
    crate::stat::fill_from_fsstat(&mut sb, &raw);
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
/// # Linux semantics (`kernel/sys.c::sys_setuid`)
///
/// ```text
/// if (kuid == cred->uid || kuid == cred->euid || kuid == cred->suid ||
///     ns_capable_setid(ns, CAP_SETUID))
///     return commit_creds(new);
/// return -EPERM;
/// ```
///
/// The kernel allows the syscall when the requested uid matches *any*
/// of the caller's real/effective/saved uids (in which case no
/// capability is needed — the call is just toggling between identities
/// the caller already legitimately holds), or when the caller holds
/// `CAP_SETUID`.  Anything else fails with `EPERM`.
///
/// Our process model is single-user: real, effective, and saved uid
/// are all `0` ("root").  That collapses the three matching arms into
/// "uid == 0", so:
///
/// * `setuid(0)`  — always succeeds, no cap required (target equals
///                  current).
/// * `setuid(N)` with `N != 0` — requires `CAP_SETUID`, else `EPERM`.
///
/// **Phase 192:** pre-Phase-192 we returned `0` for *every* uid value,
/// ignoring caps.  That let an unprivileged sandbox call
/// `setuid(1000)` and continue to think it had successfully dropped
/// privilege when in fact nothing changed — exactly the kind of silent
/// "permission boundary skipped" bug containers care about.  The cap
/// gate now matches Linux's behaviour for the typical "drop to
/// nobody / unprivileged user" use case: callers without CAP_SETUID
/// see EPERM and surface the misconfiguration loudly.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setuid(uid: UidT) -> i32 {
    // Phase 192: target uid != current → needs CAP_SETUID.  Our
    // current uid is always 0, so any non-zero target requires the
    // cap.  Linux's `sys_setuid` also accepts the call when uid
    // matches the effective or saved uid; in our flat model those
    // are also 0, so the same "uid == 0 always OK" rule covers all
    // three Linux match arms simultaneously.
    if uid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETUID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    0
}

/// Set the effective user ID of the calling process.
///
/// # Linux semantics (`kernel/sys.c::sys_setreuid` / `sys_seteuid`)
///
/// `seteuid(uid)` is a thin wrapper around `setreuid(-1, uid)` in
/// glibc; the kernel implements both via the same permission table.
/// For the effective-uid field specifically:
///
/// ```text
/// if (euid != (uid_t)-1 &&
///     euid != cred->uid && euid != cred->euid && euid != cred->suid &&
///     !ns_capable_setid(ns, CAP_SETUID))
///     return -EPERM;
/// ```
///
/// Match-against-any-current-uid OR `CAP_SETUID`.  Same collapse to
/// "uid == 0" in our single-user model.
///
/// **Phase 192:** the previous "succeeds silently" stub had the same
/// hole as [`setuid`] — `seteuid(1000)` looked like it dropped
/// privilege but did nothing.  The cap gate restores Linux's EPERM on
/// the privilege-boundary path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn seteuid(uid: UidT) -> i32 {
    // Phase 192: same rule as setuid — target == 0 always OK; non-zero
    // requires CAP_SETUID.
    if uid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETUID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    0
}

/// Set the group ID of the calling process.
///
/// # Linux semantics (`kernel/sys.c::sys_setgid`)
///
/// Linux's `sys_setgid` allows the call when the target gid matches
/// the real, effective, or saved gid OR the caller holds `CAP_SETGID`;
/// otherwise `-EPERM`.  This is the gid analogue of `setuid`'s rule.
///
/// In our flat single-gid (always 0) model the three match arms
/// collapse to "target == 0 always OK; target != 0 requires
/// CAP_SETGID".
///
/// **Phase 193:** pre-Phase-193 the stub returned `0` for every gid
/// value, mirroring the silent-success bug Phase 192 fixed for
/// `setuid`.  An unprivileged caller could call `setgid(1000)` and
/// believe it had successfully dropped to an unprivileged group while
/// in fact nothing had changed.  The cap gate restores Linux's EPERM
/// for the privilege-boundary path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setgid(gid: GidT) -> i32 {
    // Phase 193: target gid != current → needs CAP_SETGID.  Our
    // current gid is always 0, so any non-zero target requires the
    // cap.  Linux also accepts the call when gid matches the
    // effective or saved gid; in our flat model those are also 0,
    // so the same "gid == 0 always OK" rule covers all three Linux
    // match arms simultaneously.
    if gid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETGID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    0
}

/// Set the effective group ID of the calling process.
///
/// # Linux semantics (`kernel/sys.c::sys_setregid` / `sys_setegid`)
///
/// `setegid(gid)` is a thin wrapper around `setregid(-1, gid)` in
/// glibc; the kernel implements both via the same permission table.
/// For the effective-gid field:
///
/// ```text
/// if (egid != (gid_t)-1 &&
///     egid != cred->gid && egid != cred->egid && egid != cred->sgid &&
///     !ns_capable_setid(ns, CAP_SETGID))
///     return -EPERM;
/// ```
///
/// Same collapse to "gid == 0" in our single-group model.
///
/// **Phase 193:** the previous "succeeds silently" stub had the same
/// hole as [`setgid`] — `setegid(1000)` looked like it changed the
/// effective gid but did nothing.  The cap gate restores Linux's
/// EPERM on the privilege-boundary path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setegid(gid: GidT) -> i32 {
    // Phase 193: same rule as setgid — target == 0 always OK; non-zero
    // requires CAP_SETGID.
    if gid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETGID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    0
}

/// Set the real and effective user IDs.
///
/// # Linux semantics (`kernel/sys.c::sys_setreuid`)
///
/// Each field is independently permission-checked.  A value of
/// `(uid_t)-1` (= `UidT::MAX` here) means "leave this field alone"
/// and bypasses its check entirely.  For the ruid field:
///
/// ```text
/// if (ruid_set &&                                /* ruid != -1 */
///     !uid_eq(old->uid, new_ruid) &&
///     !uid_eq(old->euid, new_ruid) &&
///     !ns_capable_setid(ns, CAP_SETUID))
///     return -EPERM;
/// ```
///
/// For the euid field the match list also includes `suid`.  In our
/// flat single-uid (always 0) model both arms collapse to "value ==
/// 0 or value == -1 always OK; any other value requires CAP_SETUID".
///
/// **Phase 194:** pre-Phase-194 we returned `0` for every (ruid,
/// euid) pair, masking the same silent privilege-skip bug Phase 192
/// fixed for `setuid`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setreuid(ruid: UidT, euid: UidT) -> i32 {
    // Phase 194: -1 sentinel skips the check; otherwise the field
    // must be 0 (matches current uid) or the caller must hold
    // CAP_SETUID.  Linux evaluates ruid before euid; either failing
    // returns EPERM with no partial state change (our stub has no
    // persisted state to roll back).
    if ruid != UidT::MAX
        && ruid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETUID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    if euid != UidT::MAX
        && euid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETUID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    0
}

/// Set the real and effective group IDs.
///
/// # Linux semantics (`kernel/sys.c::sys_setregid`)
///
/// Mirror of [`setreuid`] for gids.  Each field independently
/// permission-checked; `(gid_t)-1` (= `GidT::MAX`) means "leave
/// alone".  Gated by `CAP_SETGID`.
///
/// **Phase 194:** previous stub silently succeeded for any pair.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setregid(rgid: GidT, egid: GidT) -> i32 {
    if rgid != GidT::MAX
        && rgid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETGID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    if egid != GidT::MAX
        && egid != 0
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SETGID,
        )
    {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    0
}

/// Get the supplementary group IDs.
///
/// Linux semantics (kernel/groups.c::SYSCALL_DEFINE2(getgroups)):
/// * `size < 0` → `-EINVAL`.
/// * `size == 0` → return the number of supplementary groups
///   without touching `list` (the query form).
/// * `size > 0` and our supplementary group count fits → copy the
///   list and return the count.
///
/// We have no supplementary groups, so once the prologue passes we
/// always return 0.  `list` is never dereferenced because there is
/// nothing to copy — matching Linux's behaviour, which only invokes
/// `copy_to_user` when `ngroups > 0`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgroups(size: i32, _list: *mut GidT) -> i32 {
    if size < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Set the supplementary group IDs.
///
/// Linux validation order (`kernel/groups.c::SYSCALL_DEFINE2(setgroups,
/// int, gidsetsize, gid_t __user *, grouplist)`):
///
///   1. **Phase 187:** `!may_setgroups()` → `-1` with `EPERM`.
///      `may_setgroups()` is `ns_capable_setid(user_ns, CAP_SETGID) &&
///      userns_may_setgroups(user_ns)`.  In our single-user-namespace
///      model the userns check collapses to true, so the gate reduces
///      to a pure `CAP_SETGID` probe.  The cap check runs *first* —
///      before any argument validation — so an unprivileged caller
///      passing garbage arguments still sees `EPERM`, never `EINVAL`
///      or `EFAULT`.  Pre-Phase-187 we silently accepted every
///      well-formed call regardless of privilege, which let
///      unprivileged code freely reshape its supplementary group list
///      and broke `setgroups`-based privilege separation (the classic
///      `setgroups(0, NULL)` drop idiom that container runtimes,
///      `su`/`sudo`, and the OpenSSH daemon all rely on).
///   2. `size > NGROUPS_MAX` (65536) → `-1` with `EINVAL`.
///   3. `size > 0 && list == NULL` → `-1` with `EFAULT` (Linux:
///      `groups_from_user`'s `copy_from_user` on a NULL grouplist).
///   4. `size == 0` succeeds regardless of `list` (drops all
///      supplementary groups; Linux explicitly permits a NULL `list`
///      when `size == 0`).
///   5. Otherwise: accepted as a no-op success.  No per-gid validation
///      (Linux accepts any gid_t value here; range/policy enforcement
///      happens at the LSM layer which we don't model).
///
/// Note: our `_SC_NGROUPS_MAX` advertises 32 (the POSIX minimum
/// guarantee), but Linux's kernel ceiling is 65536 and we accept up
/// to that for binary-compat parity with programs probing the kernel
/// limit directly.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setgroups(size: usize, list: *const GidT) -> i32 {
    const NGROUPS_KERNEL_MAX: usize = 65536;
    // Phase 187: CAP_SETGID gate runs first, matching Linux's
    // `may_setgroups()` placement at the top of the syscall handler.
    // The previous "we're single-user, no EPERM path" doctring was
    // stale — capabilities have existed since the cred-model phases,
    // and Linux's order is cap-check-then-validate.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SETGID,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    if size > NGROUPS_KERNEL_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if size > 0 && list.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    0
}

/// Check if the process is running setuid or setgid.
///
/// Returns 1 if the process was started with elevated privileges
/// (real uid != effective uid, or real gid != effective gid), 0
/// otherwise.  Since our OS is single-user and always runs as root,
/// this always returns 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn issetugid() -> i32 {
    // Single-user OS: uid/gid are always 0/0.
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

/// Copy the current hostname into `out`, returning the number of bytes
/// written (excluding any null terminator).  Truncates if `out` is
/// smaller than the stored hostname; never null-terminates — the caller
/// owns null-termination semantics.
///
/// Used by `utsname::uname()` so the utsname `nodename` field reflects
/// the same hostname that `gethostname()` / `sethostname()` see, instead
/// of a hardcoded "localhost".
pub(crate) fn copy_hostname(out: &mut [u8]) -> usize {
    // SAFETY: Single-address-space, no concurrent writes during the read.
    // Same access pattern as `gethostname()` above.
    let (src_ptr, src_len) = unsafe {
        (&raw const HOSTNAME_BUF, HOSTNAME_LEN)
    };
    let n = core::cmp::min(out.len(), src_len);
    let mut i = 0;
    while i < n {
        // SAFETY: i < HOSTNAME_LEN <= HOST_NAME_MAX, HOSTNAME_BUF is at
        // least HOST_NAME_MAX + 1 bytes, and out[i] is in-bounds because
        // i < n <= out.len().
        unsafe {
            let b = *src_ptr.cast::<u8>().add(i);
            if let Some(slot) = out.get_mut(i) {
                *slot = b;
            }
        }
        i = i.wrapping_add(1);
    }
    n
}

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
/// Errors (Linux-matching priority order — `kernel/sys.c::sys_setdomainname`
/// performs the cap check at the top, before argument validation):
///
/// 1. `!CAP_SYS_ADMIN`              → `EPERM`   (Phase 167)
/// 2. `len > HOST_NAME_MAX`          → `EINVAL`
/// 3. `name == NULL` when `len > 0`  → `EFAULT`
///
/// `len == 0` is accepted even with a NULL pointer (matches Linux's
/// `copy_from_user(_, NULL, 0)` short-circuit).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setdomainname(name: *const u8, len: usize) -> i32 {
    // 1. CAP_SYS_ADMIN check first (Phase 167).  The comment that
    //    previously said "we're single-user so any process can set
    //    the domain name" predates the cred model — now that
    //    capabilities exist, Linux's sys_setdomainname ordering
    //    applies.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_ADMIN,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    if len > HOST_NAME_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if len > 0 && name.is_null() {
        errno::set_errno(errno::EFAULT);
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
            // On bare metal, query the kernel for the online CPU count.
            // On host (cargo test), fall back to 1 — the host's syscall
            // table doesn't match ours.
            #[cfg(target_os = "none")]
            {
                let n = crate::syscall::syscall0(crate::syscall::SYS_CPU_COUNT);
                if n >= 1 { n } else { 1 }
            }
            #[cfg(not(target_os = "none"))]
            {
                1
            }
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
        _SC_PHYS_PAGES => {
            // 16 KiB pages — kernel returns the count of 16 KiB frames it manages.
            #[cfg(target_os = "none")]
            {
                let n = crate::syscall::syscall0(crate::syscall::SYS_PHYS_PAGES_TOTAL);
                if n >= 1 { n } else { 1 }
            }
            #[cfg(not(target_os = "none"))]
            { 8192 }                    // host test fallback (~128 MiB at 16 KiB pages)
        }
        _SC_AVPHYS_PAGES => {
            #[cfg(target_os = "none")]
            {
                let n = crate::syscall::syscall0(crate::syscall::SYS_PHYS_PAGES_AVAIL);
                if n >= 0 { n } else { 0 }
            }
            #[cfg(not(target_os = "none"))]
            { 4096 }                    // host test fallback (~64 MiB at 16 KiB pages)
        }
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
/// Same as pathconf but takes a file descriptor.  Validates `fd` —
/// Linux returns -1/EBADF for a closed fd before any name lookup —
/// then delegates to `pathconf` for the actual value table.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EINVAL` — `name` is not a recognised `_PC_*` constant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fpathconf(fd: i32, name: i32) -> i64 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
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
        errno::set_errno(errno::EFAULT);
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

/// Resolve a pathname to an absolute path (GNU extension).
///
/// Equivalent to `realpath(path, NULL)` — allocates a buffer via `malloc`
/// and writes the resolved path into it.  The caller must `free()` the
/// returned pointer.
///
/// Returns a `malloc`'d null-terminated string on success, or null on
/// error with errno set.
///
/// # Safety
///
/// `path` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn canonicalize_file_name(path: *const u8) -> *mut u8 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }

    // Use a stack buffer for resolution, then copy to a malloc'd buffer.
    let mut resolved = [0u8; PATH_MAX];
    let Some(resolved_len) = (unsafe { resolve_path(path, &mut resolved) }) else {
        if unsafe { *path } == 0 {
            errno::set_errno(errno::ENOENT);
        } else {
            errno::set_errno(errno::ENAMETOOLONG);
        }
        return core::ptr::null_mut();
    };

    // Verify the path exists (like realpath).
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

    // Allocate buffer for the resolved path (+1 for null terminator).
    let buf = crate::malloc::malloc(resolved_len.wrapping_add(1));
    if buf.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }

    // Copy the resolved path and null-terminate.
    // SAFETY: buf is valid for resolved_len+1 bytes; resolved is valid.
    unsafe {
        core::ptr::copy_nonoverlapping(
            resolved.as_ptr(),
            buf,
            resolved_len,
        );
        *buf.add(resolved_len) = 0;
    }

    buf
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
/// Errors (Linux-matching priority order — `kernel/sys.c::sys_sethostname`
/// checks the cap first, then the length, then the user pointer):
///
/// 1. `!CAP_SYS_ADMIN`              → `EPERM`   (Phase 167)
/// 2. `len > HOST_NAME_MAX`          → `EINVAL`
/// 3. `name == NULL` when `len > 0`  → `EFAULT`
///
/// `len == 0` is accepted even with a NULL pointer (matches Linux's
/// `copy_from_user(_, NULL, 0)` short-circuit).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sethostname(name: *const u8, len: usize) -> i32 {
    // 1. CAP_SYS_ADMIN check first (Phase 167) — Linux's
    //    sys_sethostname checks ns_capable(uts_ns->user_ns,
    //    CAP_SYS_ADMIN) at the top of the syscall prologue,
    //    before any argument validation.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_ADMIN,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    // 2. Length sanity (matches Linux's `len < 0 || len >
    //    __NEW_UTS_LEN` check; our `usize` parameter already
    //    excludes negative values).
    if len > HOST_NAME_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // 3. Empty copy never dereferences the pointer — Linux's
    //    copy_from_user(_, NULL, 0) returns 0 so sethostname(NULL, 0)
    //    succeeds.  We mirror that and only EFAULT on len > 0.
    if len > 0 && name.is_null() {
        errno::set_errno(errno::EFAULT);
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

// ---------------------------------------------------------------------------
// gethostid / sethostid — host identifier
// ---------------------------------------------------------------------------

/// Process-local host identifier.
///
/// Initialized to 0 (= "unset" sentinel).  `sethostid()` writes here.
/// When unset, `gethostid()` derives a value from the current hostname
/// via FNV-1a so callers get a stable-per-hostname 32-bit identifier
/// instead of always seeing 0 (which would defeat the function's
/// purpose of distinguishing hosts).
///
/// Real Linux persists the value in `/etc/hostid`; we have no on-disk
/// store yet so it lives in memory only, lost across reboots.  Once
/// the OS gains a proper config-files directory, this should migrate
/// to a file under `/etc/hostid` to match Linux semantics.
static HOSTID: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(0);

/// FNV-1a 32-bit hash — used to derive a stable hostid from the hostname
/// when no explicit hostid was set via `sethostid()`.
fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < bytes.len() {
        // Index via .get() to keep clippy::indexing_slicing happy and
        // because the loop bound has already been checked.
        if let Some(&b) = bytes.get(i) {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// Get the unique identifier of the current host.
///
/// Returns the value previously set by `sethostid()` if any, otherwise
/// derives a stable 32-bit identifier from the current hostname via
/// FNV-1a so callers see a deterministic non-zero value.  POSIX defines
/// the return type as `long` (`i64` on LP64); the value is conceptually
/// 32 bits and we sign-extend through i64.
///
/// Never fails.  Used by programs that want a coarse machine identifier
/// (e.g. inetd's anti-replay heuristics, distributed lockfile owner IDs,
/// `tar`'s archive UUID seed).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gethostid() -> i64 {
    use core::sync::atomic::Ordering;

    let stored = HOSTID.load(Ordering::Relaxed);
    if stored != 0 {
        return stored;
    }

    // Derive from hostname.  SAFETY: single-address-space, no concurrent
    // writes during read (matches the gethostname / copy_hostname pattern).
    let (src_ptr, src_len) = unsafe { (&raw const HOSTNAME_BUF, HOSTNAME_LEN) };
    let mut tmp = [0u8; HOST_NAME_MAX];
    let n = core::cmp::min(src_len, tmp.len());
    let mut i = 0;
    while i < n {
        // SAFETY: i < n <= HOSTNAME_LEN <= HOST_NAME_MAX, source buffer is
        // HOST_NAME_MAX + 1 bytes, and tmp has HOST_NAME_MAX bytes.
        unsafe {
            let b = *src_ptr.cast::<u8>().add(i);
            if let Some(slot) = tmp.get_mut(i) {
                *slot = b;
            }
        }
        i = i.wrapping_add(1);
    }

    let h = fnv1a32(tmp.get(..n).unwrap_or(&[]));
    // Sign-extend through i32 → i64 so a value with the high bit set
    // (e.g. fnv1a32("localhost") = 0xc2e09d09) doesn't appear as a
    // huge positive number to callers that expect `(int)gethostid()`.
    #[allow(clippy::cast_possible_wrap)]
    let signed = i64::from(h as i32);
    signed
}

/// Set the unique identifier of the current host.
///
/// Stores `hostid` in process memory.  Subsequent calls to `gethostid()`
/// will return this value (sign-extended through i32 to match the
/// historical `(int)` cast in glibc).  Real Linux requires CAP_SYS_ADMIN
/// and persists to `/etc/hostid`; we have neither user model nor on-disk
/// config files yet, so we accept any caller and persist only in RAM.
///
/// Returns 0 on success.  Never fails on this platform.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sethostid(hostid: i64) -> i32 {
    use core::sync::atomic::Ordering;

    // Truncate to 32 bits and sign-extend back, matching Linux's
    // `sethostid(int)` historical behaviour even though our prototype
    // takes `long`.  Callers that pass a 64-bit value will see the low
    // 32 bits round-trip.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let truncated = i64::from(hostid as i32);
    // If the caller passes 0, store a sentinel that's not zero (we use 0
    // internally to mean "unset" so the FNV fallback kicks in).  Pick
    // the FNV-derived value of the current hostname so the round-trip
    // through 0 is a no-op: gethostid() before sethostid(0) and after
    // both return the same hostname-derived hash.
    let to_store = if truncated == 0 { gethostid() } else { truncated };
    HOSTID.store(to_store, Ordering::Relaxed);
    0
}

/// Change the root directory.
///
/// Stub: validates arguments per Linux `fs/open.c::sys_chroot`, then
/// returns `-1` with `ENOSYS` (filesystem-root remapping isn't wired
/// up yet — `design.txt` puts root selection in the capability layer
/// rather than via legacy chroot semantics).
///
/// Errors (Linux-matching priority order — `fs/open.c::sys_chroot`
/// resolves the user pointer with `user_path_at` *before* checking
/// `CAP_SYS_CHROOT`, so path-domain errors beat EPERM):
///
/// 1. `path == NULL`           → `EFAULT`
/// 2. `*path == 0`             → `ENOENT`  (Linux's `getname_kernel`
///                                rejects the empty-name path)
/// 3. `!CAP_SYS_CHROOT`        → `EPERM`   (Phase 166)
///
/// After argument and capability validation we return `ENOSYS`:
/// filesystem-root remapping isn't wired up yet — `design.txt`
/// puts root selection in the capability layer rather than via
/// legacy chroot semantics.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chroot(path: *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: path was just confirmed non-NULL; we read one byte to
    // distinguish the empty-string case from a real path.  Caller's
    // contract guarantees the buffer is at least NUL-terminated.
    let first = unsafe { *path };
    if first == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    // Phase 166: CAP_SYS_CHROOT check.  Linux performs this after
    // user_path_at + inode_permission, so path-domain errors still
    // beat EPERM.  We don't yet do inode_permission, so EPERM
    // immediately follows the path-syntax checks.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_CHROOT,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
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
        let v = {
            #[cfg(target_os = "none")]
            {
                // Kernel FSHIFT — must match crate::loadavg::FSHIFT (11).
                const FIXED_1_F: f64 = 2048.0;
                let raw = crate::syscall::syscall1(
                    crate::syscall::SYS_LOADAVG,
                    i as u64,
                );
                // Negative = kernel error (shouldn't happen for 0..=2);
                // treat as zero load.
                if raw < 0 { 0.0 } else { raw as f64 / FIXED_1_F }
            }
            #[cfg(not(target_os = "none"))]
            { 0.0 }
        };
        // SAFETY: loadavg is valid for at least nelem elements (caller
        // contract), and i < count <= nelem.
        unsafe {
            *loadavg.add(i as usize) = v;
        }
        i = i.wrapping_add(1);
    }

    count
}

// ---------------------------------------------------------------------------
// getrandom / getentropy
// ---------------------------------------------------------------------------

/// Flags for `getrandom`.
pub const GRND_NONBLOCK: u32 = 0x0001;
/// Use the random source (not urandom).
pub const GRND_RANDOM: u32 = 0x0002;
/// Use the insecure (non-blocking, non-validated) entropy source.
///
/// Linux 5.17+ flag.  `GRND_INSECURE` is mutually exclusive with
/// `GRND_RANDOM` — passing both returns `EINVAL`.
pub const GRND_INSECURE: u32 = 0x0004;

/// Mask of all flag bits accepted by `getrandom`.
///
/// Any bit set outside this mask causes `getrandom` to return `EINVAL`,
/// matching Linux's kernel-side validation.
pub const GRND_VALID_FLAGS: u32 = GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE;

/// Fill a buffer with random bytes.
///
/// Uses `rdrand` x86_64 instruction where available.  Falls back to
/// a simple LCG seeded from the monotonic clock if RDRAND fails.
///
/// # Flag validation
///
/// Matches the Linux kernel's `SYSCALL_DEFINE3(getrandom, ...)` prologue:
///
/// 1. `flags & ~GRND_VALID_FLAGS != 0` → `EINVAL` (unknown flag bit).
/// 2. `flags & (GRND_RANDOM|GRND_INSECURE) == (GRND_RANDOM|GRND_INSECURE)`
///    → `EINVAL` (mutually exclusive flags).
/// 3. `buf` null with non-zero `buflen` → `EFAULT` (matches Linux's
///    `copy_to_user` failure mode).
/// 4. `buflen > isize::MAX` → `EINVAL` (local guard so the return value
///    cannot be misread as an error).
///
/// On success returns the number of bytes filled (always `buflen` in our
/// implementation — we never short-read).
///
/// Returns the number of bytes filled, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrandom(buf: *mut u8, buflen: usize, flags: u32) -> isize {
    // 1. Unknown flag bits → EINVAL.  This check comes before any buffer
    //    inspection, matching Linux: invalid flags are rejected before
    //    copy_to_user is ever called.
    if (flags & !GRND_VALID_FLAGS) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 2. GRND_RANDOM and GRND_INSECURE are mutually exclusive.
    if (flags & (GRND_RANDOM | GRND_INSECURE)) == (GRND_RANDOM | GRND_INSECURE) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 3. A null buffer with non-zero length is a fault.  Linux reports
    //    this via copy_to_user → EFAULT.  A zero-length call with a null
    //    buffer is allowed (and returns 0) since no bytes need to move.
    if buf.is_null() {
        if buflen == 0 {
            return 0;
        }
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // 4. Guard against buflen > isize::MAX to avoid returning a negative
    //    value that callers would interpret as an error.
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
pub(crate) fn fill_random(buf: *mut u8, len: usize) {
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
// syncfs
// ---------------------------------------------------------------------------

// NOTE: fdatasync() is in file.rs (delegates to fsync).

/// Synchronize all data for the filesystem containing `fd`.
///
/// Validates `fd`: must be non-negative and open in the fd table.  Body
/// is a no-op success because our writes are already synchronous, but
/// the prologue catches buggy callers passing -1 or a closed fd.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn syncfs(fd: Fd) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
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

/// Size of the buffer written by `PR_GET_NAME` and read by `PR_SET_NAME`.
///
/// Matches Linux's `TASK_COMM_LEN` (16 bytes including the NUL
/// terminator).  Both `PR_SET_NAME` and `PR_GET_NAME` are defined in
/// terms of this size — `PR_SET_NAME` reads up to `TASK_COMM_LEN - 1`
/// bytes (15) and NUL-terminates; `PR_GET_NAME` writes exactly
/// `TASK_COMM_LEN` bytes via `copy_to_user(arg2, comm, sizeof(comm))`.
pub const TASK_COMM_LEN: usize = 16;

/// One-way "no new privileges" flag (Phase 160).
///
/// Tracks Linux's `task->no_new_privs` bit.  `PR_SET_NO_NEW_PRIVS`
/// flips it to `true`; `PR_GET_NO_NEW_PRIVS` reads it.  Linux defines
/// the flag as **one-way**: once set, it cannot be cleared except by a
/// fresh `execve` (which doesn't apply to our single-process model).
///
/// Stored as an `AtomicBool` to match the lock-free fast-path that
/// real seccomp/sandbox code expects — every syscall on Linux that
/// honours `no_new_privs` reads this bit unguarded.  Pre-Phase-160 the
/// flag was discarded entirely (SET succeeded but GET always returned
/// 0), which silently disabled the security semantics that callers
/// like Chromium's sandbox and Docker's `--security-opt=no-new-privileges`
/// rely on.
static NO_NEW_PRIVS: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Read the current `no_new_privs` bit (Phase 160).
///
/// Exposed as a `pub fn` so other parts of the posix layer can gate
/// privilege-raising operations on it without going through `prctl`.
/// Linux's seccomp, execve, and capability-set machinery all use the
/// kernel-internal `task_no_new_privs()` accessor for the same purpose.
#[must_use]
pub fn no_new_privs_set() -> bool {
    NO_NEW_PRIVS.load(core::sync::atomic::Ordering::Relaxed)
}

/// Test-only reset of the `no_new_privs` bit.
///
/// `PR_SET_NO_NEW_PRIVS` is irreversible in Linux (and in our prctl
/// implementation, by design) — once set, it stays set for the life
/// of the task.  But tests in other modules (e.g. landlock,
/// seccomp) need to observe both the cleared and set states of the
/// bit to exercise the `task_no_new_privs() || CAP_SYS_ADMIN`
/// branches in their syscall stubs.  Direct atomic access stays
/// inside this module; cross-module test code goes through this
/// accessor, which is gated on `cfg(test)` so it cannot leak into
/// production builds.
#[cfg(test)]
pub(crate) fn _test_reset_no_new_privs(value: bool) {
    NO_NEW_PRIVS.store(value, core::sync::atomic::Ordering::Relaxed);
}

/// Process control operations (Linux).
///
/// Stub: implements `PR_SET_NAME` / `PR_GET_NAME` as a name buffer
/// pass-through and `PR_SET_NO_NEW_PRIVS` / `PR_GET_NO_NEW_PRIVS` as
/// trivial accept-and-return operations.  All other options return
/// `-1` with `EINVAL`.
///
/// Argument-domain validation matches `kernel/sys.c::sys_prctl` in
/// Linux:
///
/// * `PR_SET_NAME` (15) / `PR_GET_NAME` (16):
///   - `arg2 == 0` → `EFAULT` (Linux's `copy_{from,to}_user` faults
///     on a NULL pointer).
///   - `arg3`, `arg4`, `arg5` are *not* checked here.  Linux ignores
///     extra args for these two opcodes (the names predate the strict
///     "all extra args must be 0" convention introduced in 2.6.x).
///   - **Phase 159:** `PR_GET_NAME` writes **all 16 bytes**
///     (`TASK_COMM_LEN`) via `copy_to_user(arg2, comm, sizeof(comm))`.
///     Pre-Phase-159 we only wrote a single NUL byte at `arg2[0]`,
///     which left `arg2[1..16]` containing whatever uninitialised stack
///     contents the caller passed in.  Linux callers (notably libc's
///     `pthread_getname_np`) expect a NUL-padded 16-byte buffer and
///     copy the whole region into their own storage; the partial write
///     leaked caller stack into the visible name.
///
/// * `PR_SET_NO_NEW_PRIVS` (38):
///   - `arg2 != 1` → `EINVAL`.  The flag is one-way; only the value
///     `1` is accepted (setting it back to `0` is impossible by design).
///   - `arg3 != 0 || arg4 != 0 || arg5 != 0` → `EINVAL`.
///   - **Phase 160:** the bit is now persisted in the `NO_NEW_PRIVS`
///     atomic so a follow-up `PR_GET_NO_NEW_PRIVS` observes the new
///     state.  Pre-Phase-160 SET succeeded but GET always returned 0,
///     silently disabling sandbox callers (Chromium, Docker
///     `--security-opt=no-new-privileges`).
///
/// * `PR_GET_NO_NEW_PRIVS` (39):
///   - `arg2 != 0 || arg3 != 0 || arg4 != 0 || arg5 != 0` → `EINVAL`.
///   - On success, returns the current bit (Phase 160: 0 or 1 based on
///     the `NO_NEW_PRIVS` atomic — was always 0 pre-fix).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32 {
    match option {
        PR_SET_NAME => {
            // NULL buffer would fault inside Linux's copy_from_user.
            if arg2 == 0 {
                crate::errno::set_errno(crate::errno::EFAULT);
                return -1;
            }
            // We don't actually store the name — accept silently.
            0
        }
        PR_GET_NAME => {
            // PR_GET_NAME writes 16 bytes; a NULL destination faults.
            if arg2 == 0 {
                crate::errno::set_errno(crate::errno::EFAULT);
                return -1;
            }
            // Phase 159: write the full TASK_COMM_LEN-byte buffer to
            // match Linux's `copy_to_user(arg2, comm, sizeof(comm))`.
            // We don't track a per-process name, so the whole buffer is
            // zeroed.  Callers can rely on every byte being initialised.
            //
            // SAFETY: caller's prctl contract guarantees a writable
            // `TASK_COMM_LEN`-byte buffer at `arg2` for `PR_GET_NAME`.
            unsafe {
                core::ptr::write_bytes(arg2 as *mut u8, 0, TASK_COMM_LEN);
            }
            0
        }
        PR_SET_NO_NEW_PRIVS => {
            // Linux: only arg2 == 1 is accepted; arg3..5 must be zero.
            if arg2 != 1 || arg3 != 0 || arg4 != 0 || arg5 != 0 {
                crate::errno::set_errno(crate::errno::EINVAL);
                return -1;
            }
            // Phase 160: persist the one-way bit so a follow-up
            // PR_GET_NO_NEW_PRIVS observes it.  Pre-fix this was a
            // silent no-op which broke sandbox callers.
            NO_NEW_PRIVS.store(true, core::sync::atomic::Ordering::Relaxed);
            0
        }
        PR_GET_NO_NEW_PRIVS => {
            // Linux: all extra args must be zero.
            if arg2 != 0 || arg3 != 0 || arg4 != 0 || arg5 != 0 {
                crate::errno::set_errno(crate::errno::EINVAL);
                return -1;
            }
            // Phase 160: report the persisted bit (was always 0 pre-fix).
            i32::from(NO_NEW_PRIVS.load(core::sync::atomic::Ordering::Relaxed))
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
/// # Linux semantics (`kernel/sys.c::sys_setresuid`)
///
/// Each of `ruid`, `euid`, `suid` is independently permission-checked.
/// A value of `(uid_t)-1` (= `UidT::MAX`) means "leave this field
/// alone" and bypasses its check.  Linux uses a single CAP_SETUID
/// fast-path that skips all three field checks if the cap is held:
///
/// ```text
/// if (!ns_capable_setid(old->user_ns, CAP_SETUID)) {
///     if (ruid != (uid_t)-1 && !uid_eq(kruid, old->uid) &&
///         !uid_eq(kruid, old->euid) && !uid_eq(kruid, old->suid))
///         return -EPERM;
///     if (euid != (uid_t)-1 && !uid_eq(keuid, old->uid) &&
///         !uid_eq(keuid, old->euid) && !uid_eq(keuid, old->suid))
///         return -EPERM;
///     if (suid != (uid_t)-1 && !uid_eq(ksuid, old->uid) &&
///         !uid_eq(ksuid, old->euid) && !uid_eq(ksuid, old->suid))
///         return -EPERM;
/// }
/// ```
///
/// In our flat single-uid (always 0) model each non-sentinel field
/// must be 0 (matches current uid/euid/suid) OR the caller must
/// hold CAP_SETUID.  Order: ruid → euid → suid (matches Linux).
///
/// **Phase 195:** pre-Phase-195 returned `0` for every triple,
/// continuing the silent-success bug pattern Phases 192-194 fixed
/// for setuid/seteuid/setreuid.  Worth flagging because setresuid
/// is the syscall sandbox/jail code uses to clamp all three uids
/// simultaneously — a silent no-op here was particularly dangerous.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setresuid(ruid: UidT, euid: UidT, suid: UidT) -> i32 {
    // Phase 195: short-circuit on CAP_SETUID — if held, all three
    // fields are accepted as-is (matches Linux's outer `if (!cap)`
    // guard).  Otherwise check each non-sentinel field against the
    // current uid (always 0 in our flat model).
    if crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SETUID,
    ) {
        return 0;
    }
    if ruid != UidT::MAX && ruid != 0 {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    if euid != UidT::MAX && euid != 0 {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    if suid != UidT::MAX && suid != 0 {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    0
}

/// Set real, effective, and saved set-group-ID.
///
/// # Linux semantics (`kernel/sys.c::sys_setresgid`)
///
/// Mirror of [`setresuid`] for gids.  Gated by `CAP_SETGID`.  Each
/// field of `(rgid, egid, sgid)` independently checked; `(gid_t)-1`
/// (= `GidT::MAX`) means "leave alone".
///
/// **Phase 195:** previous stub silently succeeded.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setresgid(rgid: GidT, egid: GidT, sgid: GidT) -> i32 {
    if crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SETGID,
    ) {
        return 0;
    }
    if rgid != GidT::MAX && rgid != 0 {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    if egid != GidT::MAX && egid != 0 {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
    if sgid != GidT::MAX && sgid != 0 {
        crate::errno::set_errno(crate::errno::EPERM);
        return -1;
    }
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
// sysinfo — system information
// ---------------------------------------------------------------------------

/// System information structure (Linux `struct sysinfo`).
#[repr(C)]
pub struct Sysinfo {
    /// Seconds since boot.
    pub uptime: i64,
    /// 1, 5, and 15 minute load averages (scaled by 65536).
    pub loads: [u64; 3],
    /// Total usable main memory size (bytes).
    pub totalram: u64,
    /// Available memory size (bytes).
    pub freeram: u64,
    /// Amount of shared memory (bytes).
    pub sharedram: u64,
    /// Memory used by buffers (bytes).
    pub bufferram: u64,
    /// Total swap space size (bytes).
    pub totalswap: u64,
    /// Swap space still available (bytes).
    pub freeswap: u64,
    /// Number of current processes.
    pub procs: u16,
    /// Padding.
    _pad: [u8; 6],
    /// Total high memory size (bytes).
    pub totalhigh: u64,
    /// Available high memory size (bytes).
    pub freehigh: u64,
    /// Memory unit size in bytes.
    pub mem_unit: u32,
    /// Padding to 64 bytes.
    _padding: [u8; 4],
}

/// Read the kernel's live process count for `sysinfo.procs`.
///
/// On bare metal, queries `SYS_PROCESS_COUNT` and clamps the result to
/// `u16` (the Linux `struct sysinfo.procs` field is unsigned short).
/// Negative kernel returns are treated as zero (defensive — the syscall
/// is documented as never failing, but the i64 carries the encoded-error
/// convention everywhere else, so we mirror it).
///
/// On host builds (cargo test), returns a fixed 1 so unit tests stay
/// deterministic.
#[cfg(target_os = "none")]
fn read_process_count() -> u16 {
    let raw = syscall0(SYS_PROCESS_COUNT);
    if raw <= 0 {
        return 0;
    }
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let n = raw as u64;
    if n > u64::from(u16::MAX) { u16::MAX } else { n as u16 }
}

#[cfg(not(target_os = "none"))]
fn read_process_count() -> u16 {
    1
}

/// Return overall system statistics.
///
/// Fills the `Sysinfo` structure from real kernel data on the kernel
/// target:
/// - `uptime`: seconds since boot from `SYS_CLOCK_MONOTONIC`.
/// - `loads`: 1/5/15-minute EWMA load averages from `SYS_LOADAVG`,
///   rescaled from our internal FSHIFT=11 (×2048) to Linux's FSHIFT=16
///   (×65536) by left-shifting 5 bits.
/// - `totalram` / `freeram`: physical page counts from `SYS_PHYS_PAGES_*`
///   multiplied by `mem_unit` (which is set to our 16 KiB frame size).
/// - `procs`: live process count from `SYS_PROCESS_COUNT`, capped to
///   `u16::MAX` (the Linux ABI uses `unsigned short` here).
/// - `sharedram` / `bufferram` / `totalswap` / `freeswap` / `totalhigh`
///   / `freehigh`: 0 (no swap, no buffer-cache accounting, no high-mem
///   region — we're 64-bit only).
///
/// On host builds, returns synthetic values (256 MiB total / 128 MiB free,
/// zero loads, uptime 0) so unit tests get deterministic output.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sysinfo(info: *mut Sysinfo) -> i32 {
    if info.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Get uptime from monotonic clock (nanoseconds → seconds).
    let mono_ns = syscall0(SYS_CLOCK_MONOTONIC);
    let uptime = if mono_ns > 0 { mono_ns / 1_000_000_000 } else { 0 };

    // SAFETY: info is verified non-null and points to a valid Sysinfo.
    unsafe {
        let s = &mut *info;
        s.uptime = uptime;
        s.sharedram = 0;
        s.bufferram = 0;
        s.totalswap = 0;
        s.freeswap = 0;
        s.procs = read_process_count();
        s._pad = [0; 6];
        s.totalhigh = 0;
        s.freehigh = 0;
        s._padding = [0; 4];

        #[cfg(target_os = "none")]
        {
            // mem_unit = 16384 (frame size).  Linux callers expect
            // mem_unit to scale totalram/freeram; reporting frames-as-units
            // avoids u64 multiplication and matches our page-granularity.
            const FRAME_SIZE: u32 = 16 * 1024;
            s.mem_unit = FRAME_SIZE;

            // Load averages: kernel returns FSHIFT=11 (×2048); Linux
            // sysinfo expects FSHIFT=16 (×65536).  Multiply by 32.
            #[allow(clippy::cast_sign_loss)]
            let load_for = |idx: u64| -> u64 {
                let raw = syscall1(SYS_LOADAVG, idx);
                if raw < 0 { 0 } else { (raw as u64).saturating_mul(32) }
            };
            s.loads = [load_for(0), load_for(1), load_for(2)];

            // Physical pages → totalram/freeram (counted in mem_unit-byte units).
            #[allow(clippy::cast_sign_loss)]
            let total_pages = {
                let raw = syscall0(SYS_PHYS_PAGES_TOTAL);
                if raw < 1 { 1 } else { raw as u64 }
            };
            #[allow(clippy::cast_sign_loss)]
            let free_pages = {
                let raw = syscall0(SYS_PHYS_PAGES_AVAIL);
                if raw < 0 { 0 } else { raw as u64 }
            };
            s.totalram = total_pages;
            s.freeram = free_pages;
        }

        #[cfg(not(target_os = "none"))]
        {
            s.loads = [0; 3];
            s.totalram = 256 * 1024 * 1024;
            s.freeram = 128 * 1024 * 1024;
            s.mem_unit = 1;
        }
    }

    0
}

// ---------------------------------------------------------------------------
// mntent — mount table parsing
// ---------------------------------------------------------------------------

/// Mount table entry (matches `struct mntent`).
#[repr(C)]
pub struct Mntent {
    /// Name of mounted filesystem.
    pub mnt_fsname: *mut u8,
    /// Filesystem path prefix (mount point).
    pub mnt_dir: *mut u8,
    /// Mount type.
    pub mnt_type: *mut u8,
    /// Mount options.
    pub mnt_opts: *mut u8,
    /// Dump frequency.
    pub mnt_freq: i32,
    /// Pass number for fsck.
    pub mnt_passno: i32,
}

/// Open a mount table file for reading.
///
/// Stub: returns null (our OS doesn't have /etc/mtab or /proc/mounts
/// yet).  Programs that need mount information should query the kernel
/// directly via our mount-list syscall (when implemented).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setmntent(_filename: *const u8, _type: *const u8) -> *mut u8 {
    // Return null "FILE*" — signals no mount table available.
    core::ptr::null_mut()
}

/// Read the next mount table entry.
///
/// Stub: returns null (no mount table).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getmntent(_stream: *mut u8) -> *mut Mntent {
    core::ptr::null_mut()
}

/// Thread-safe version of `getmntent`.
///
/// Stub: returns null (no mount table).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getmntent_r(
    _stream: *mut u8,
    _mntbuf: *mut Mntent,
    _buf: *mut u8,
    _buflen: i32,
) -> *mut Mntent {
    core::ptr::null_mut()
}

/// Close a mount table file.
///
/// Stub: returns 1 (success) even though we never open anything.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endmntent(_stream: *mut u8) -> i32 {
    1 // glibc always returns 1.
}

/// Check if a mount option is present.
///
/// Stub: returns null (option not found).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hasmntopt(_mnt: *const Mntent, _opt: *const u8) -> *mut u8 {
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// personality — process execution domain
// ---------------------------------------------------------------------------

/// Sentinel value passed as `persona` to *query* the current personality
/// without modifying it.  Matches Linux's `kernel/exec_domain.c::sys_personality`
/// special case: when the argument equals `0xFFFFFFFF` the kernel skips
/// the set_personality() call and returns the existing value unchanged.
pub const PERSONALITY_QUERY: u32 = 0xFFFF_FFFF;

/// Backing storage for the current process personality.  Linux models
/// this per-task in `task_struct.personality`; we approximate with a
/// process-wide atomic until our process subsystem exposes per-task
/// credentials.  Initial value `0` is `PER_LINUX` — the standard
/// Linux execution domain.
static PERSONALITY_STATE: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Read the current personality without altering it.
///
/// Internal helper used by `posix::sys_personality` tests and by the
/// process subsystem when it needs to consult the personality bits
/// (e.g. to honour `ADDR_NO_RANDOMIZE` for an `execve`).
#[must_use]
pub fn current_personality() -> u32 {
    PERSONALITY_STATE.load(core::sync::atomic::Ordering::Relaxed)
}

/// Set the process execution domain.
///
/// # Linux semantics (`kernel/exec_domain.c::sys_personality`)
///
/// ```text
/// SYSCALL_DEFINE1(personality, unsigned int, persona) {
///     unsigned int old = current->personality;
///     if (persona != 0xffffffff)
///         set_personality(persona);
///     return old;
/// }
/// ```
///
/// Three observable behaviours follow:
///
/// 1. The argument is a 32-bit `unsigned int`, so any high bits set by
///    glibc's `long`/`unsigned long` wrapper are truncated by the
///    kernel.  We model that explicitly via `as u32` to keep
///    `personality(0xFFFF_FFFF_FFFF_FFFF)` indistinguishable from
///    `personality(0xFFFF_FFFF)` — both must hit the *query* arm.
/// 2. `0xFFFFFFFF` is the query sentinel: state is unchanged and the
///    current personality is returned.
/// 3. Any other value is stored verbatim (Linux does not validate the
///    individual bits — unknown personality bits are simply remembered
///    and surface to processes that look at `/proc/self/personality`).
///    The function returns the *previous* value, never the new one.
///
/// # Return value width
///
/// glibc declares this as `int personality(unsigned long)` and casts
/// the kernel's `unsigned int` result via signed extension.  Personality
/// values fit comfortably in the low 31 bits (the highest defined bit
/// is `ADDR_LIMIT_3GB = 0x08000000`), so the `as i32` cast is safe.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn personality(persona: u64) -> i32 {
    // Truncate to 32 bits the same way the Linux syscall does — its
    // C signature is `unsigned int`, so anything in the high half is
    // silently dropped.
    let persona = persona as u32;
    let old = PERSONALITY_STATE.load(core::sync::atomic::Ordering::Relaxed);
    if persona != PERSONALITY_QUERY {
        PERSONALITY_STATE.store(persona, core::sync::atomic::Ordering::Relaxed);
    }
    old as i32
}

// ---------------------------------------------------------------------------
// ptrace — process trace
// ---------------------------------------------------------------------------

/// ptrace request codes.
pub const PTRACE_TRACEME: i32 = 0;
/// Peek at a word in the child's text area.
pub const PTRACE_PEEKTEXT: i32 = 1;
/// Peek at a word in the child's data area.
pub const PTRACE_PEEKDATA: i32 = 2;
/// Peek at a word in the child's USER area.
pub const PTRACE_PEEKUSER: i32 = 3;
/// Write a word to the child's text area.
pub const PTRACE_POKETEXT: i32 = 4;
/// Write a word to the child's data area.
pub const PTRACE_POKEDATA: i32 = 5;
/// Write a word to the child's USER area.
pub const PTRACE_POKEUSER: i32 = 6;
/// Continue the stopped child.
pub const PTRACE_CONT: i32 = 7;
/// Kill the child.
pub const PTRACE_KILL: i32 = 8;
/// Single-step the child.
pub const PTRACE_SINGLESTEP: i32 = 9;
/// Attach to a process.
pub const PTRACE_ATTACH: i32 = 16;
/// Detach from a process.
pub const PTRACE_DETACH: i32 = 17;
/// Continue and signal the child.
pub const PTRACE_SYSCALL: i32 = 24;
/// Set ptrace options.
pub const PTRACE_SETOPTIONS: i32 = 0x4200;
/// Retrieve message from the latest ptrace stop.
pub const PTRACE_GETEVENTMSG: i32 = 0x4201;
/// Retrieve signal information.
pub const PTRACE_GETSIGINFO: i32 = 0x4202;
/// Set signal information.
pub const PTRACE_SETSIGINFO: i32 = 0x4203;
/// Same as PTRACE_ATTACH but does not stop the tracee.
pub const PTRACE_SEIZE: i32 = 0x4206;
/// Stop a SEIZEd tracee.
pub const PTRACE_INTERRUPT: i32 = 0x4207;
/// Listen for a stopped tracee.
pub const PTRACE_LISTEN: i32 = 0x4208;

/// Return `true` for known `PTRACE_*` request codes that our validator
/// recognises.  Codes outside this set are rejected with `EIO` —
/// matching Linux's `kernel/ptrace.c::ptrace_request` default case,
/// which historically returns `-EIO` rather than `-EINVAL` for
/// unknown request numbers.
#[must_use]
pub fn ptrace_request_known(request: i32) -> bool {
    matches!(
        request,
        PTRACE_TRACEME
            | PTRACE_PEEKTEXT
            | PTRACE_PEEKDATA
            | PTRACE_PEEKUSER
            | PTRACE_POKETEXT
            | PTRACE_POKEDATA
            | PTRACE_POKEUSER
            | PTRACE_CONT
            | PTRACE_KILL
            | PTRACE_SINGLESTEP
            | PTRACE_ATTACH
            | PTRACE_DETACH
            | PTRACE_SYSCALL
            | PTRACE_SETOPTIONS
            | PTRACE_GETEVENTMSG
            | PTRACE_GETSIGINFO
            | PTRACE_SETSIGINFO
            | PTRACE_SEIZE
            | PTRACE_INTERRUPT
            | PTRACE_LISTEN
    )
}

/// Process trace (debugging interface).
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  A real
/// ptrace implementation requires kernel-level support for
/// breakpoints, single-step, and memory/register access that our
/// microkernel does not export yet — but invalid callers (debuggers
/// targeting nonexistent pids, callers passing garbage request codes,
/// PTRACE_TRACEME called twice) must still see Linux-matching errno
/// values so portable debuggers (gdb, strace, lldb) and crash-handler
/// libraries report failures correctly.
///
/// Validation order matches `kernel/ptrace.c::sys_ptrace` in Linux:
/// 1. Unknown request code → `EIO`.  Linux's default case in
///    `ptrace_request` returns `-EIO`, not `-EINVAL`; this is a
///    historical quirk of the interface that portable callers
///    rely on.
/// 2. `PTRACE_TRACEME`: `pid`/`addr`/`data` are ignored.  Linux
///    rejects with `EPERM` if the caller is already traced, which
///    we can't check; pass through to `ENOSYS`.
/// 3. All other requests: `pid <= 0` → `ESRCH` (no such process).
///    Linux performs this via `find_get_task_by_vpid(pid)` which
///    returns `-ESRCH` for non-positive pids.
/// 4. `!capable(CAP_SYS_PTRACE)` → `EPERM`  (Phase 200).
///    In Linux this is checked inside `ptrace_may_access()` after
///    finding the target task and checking thread-group membership.
///    We place it after the `ESRCH` guard because a non-positive pid
///    is always invalid regardless of privilege — the kernel never
///    reaches the capability check for such pids.
/// 5. All validated → `ENOSYS`.
///
/// Things we cannot validate yet (will become real checks once the
/// process subsystem exposes traced-state):
/// - `ESRCH`: target pid is not in this user's session.
/// - `EFAULT`: `addr`/`data` does not refer to a readable/writable
///   address in the tracee.
/// These are deferred with TODO comments; the function will continue
/// to surface `ENOSYS` for them until the process model is wired up.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ptrace(request: i32, pid: i32, _addr: u64, _data: u64) -> i64 {
    if !ptrace_request_known(request) {
        errno::set_errno(errno::EIO);
        return -1;
    }
    if request == PTRACE_TRACEME {
        // No further argument checks — caller traces itself.
        // TODO(ptrace): EPERM if already traced once the process
        // subsystem tracks tracer state.
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    if pid <= 0 {
        errno::set_errno(errno::ESRCH);
        return -1;
    }
    // Phase 200: CAP_SYS_PTRACE gate.  In Linux, ptrace_may_access()
    // runs after finding the target task (ESRCH already screened) and
    // checking thread-group membership.  A same-thread-group attach
    // can bypass the cap check, but we don't track thread groups yet,
    // so we gate all non-TRACEME requests uniformly.  An unprivileged
    // caller with a valid pid sees EPERM rather than ENOSYS, which is
    // the correct signal for "you don't have permission" vs. "this
    // syscall isn't implemented."
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_PTRACE,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    // TODO(ptrace): ESRCH for non-existent pid, EFAULT for bad
    // addr/data — require process-model hooks we don't have yet.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// swapon / swapoff
// ---------------------------------------------------------------------------

/// Mark high-priority swap area (bit 15 of `swapflags`).
///
/// When set, the lower bits of `swapflags` form a priority for the new
/// swap area; without this bit Linux ignores those bits and assigns a
/// default priority.
pub const SWAP_FLAG_PREFER: i32 = 0x8000;
/// Discard data on swap-in for this area (TRIM support).
pub const SWAP_FLAG_DISCARD: i32 = 0x1_0000;
/// Discard swap pages eagerly when they go free.
pub const SWAP_FLAG_DISCARD_ONCE: i32 = 0x2_0000;
/// Discard swap pages on swap-in and on free.
pub const SWAP_FLAG_DISCARD_PAGES: i32 = 0x4_0000;
/// Mask of the priority field (low 15 bits).  Valid when
/// `SWAP_FLAG_PREFER` is set.
pub const SWAP_FLAG_PRIO_MASK: i32 = 0x7FFF;

/// Bitmask of every defined `swapon` flag.  Bits outside this mask are
/// rejected with `EINVAL`.
pub const SWAP_FLAGS_VALID: i32 = SWAP_FLAG_PREFER
    | SWAP_FLAG_DISCARD
    | SWAP_FLAG_DISCARD_ONCE
    | SWAP_FLAG_DISCARD_PAGES
    | SWAP_FLAG_PRIO_MASK;

/// Enable swapping on a device.
///
/// Stub: validates arguments per Linux `mm/swapfile.c::sys_swapon`, then
/// returns `-1` with `ENOSYS`.  Our OS uses committed memory and has no
/// swap subsystem (design.txt: lazy allocation is opt-in, never silent
/// overcommit).
///
/// Errors (Linux-matching priority order — `sys_swapon` performs the
/// `CAP_SYS_ADMIN` check first, then the flag-mask check *before*
/// `getname`):
///
/// 1. `!capable(CAP_SYS_ADMIN)`        → `EPERM`   (Phase 164)
/// 2. `swapflags & ~SWAP_FLAGS_VALID`  → `EINVAL`
/// 3. `path == NULL`                   → `EFAULT`  (Linux: `getname` →
///                                        `strncpy_from_user`)
/// 4. `*path == 0`                     → `ENOENT`  (Linux: getname's
///                                        empty-name path → -ENOENT)
///
/// That means `swapon(NULL, BAD_FLAG)` returns `EINVAL`, not `EFAULT`
/// — the flag check happens before the user pointer is dereferenced —
/// and an unprivileged caller always gets `EPERM` regardless of the
/// other argument values.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn swapon(path: *const u8, swapflags: i32) -> i32 {
    // 1. CAP_SYS_ADMIN check first — Linux's sys_swapon prologue
    //    (Phase 164).  This must beat the flag-mask check so an
    //    unprivileged caller never learns whether their arguments are
    //    syntactically valid.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_ADMIN,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    // 2. Flag-mask check — Linux's sys_swapon prologue.
    if swapflags & !SWAP_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // 3. NULL path → EFAULT (getname / strncpy_from_user).
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // 4. Empty path → ENOENT.  SAFETY: path is non-NULL; we read one byte.
    if unsafe { *path } == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Disable swapping on a device.
///
/// Stub: validates arguments per Linux `mm/swapfile.c::sys_swapoff`,
/// then returns `-1` with `ENOSYS`.
///
/// Errors (Linux-matching priority order — `sys_swapoff` performs the
/// `CAP_SYS_ADMIN` check first, then `getname`):
///
/// 1. `!capable(CAP_SYS_ADMIN)` → `EPERM`  (Phase 164)
/// 2. `path == NULL`            → `EFAULT`
/// 3. `*path == 0`              → `ENOENT`
///
/// An unprivileged caller always gets `EPERM`; the user pointer is not
/// inspected.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn swapoff(path: *const u8) -> i32 {
    // 1. CAP_SYS_ADMIN check first — Linux's sys_swapoff prologue
    //    (Phase 164).  Must beat the user-pointer fault so an
    //    unprivileged caller never learns whether `path` is mapped.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_ADMIN,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: path is non-NULL.
    if unsafe { *path } == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// klogctl — kernel log control
// ---------------------------------------------------------------------------

/// Close the kernel log (currently a no-op on Linux).
pub const SYSLOG_ACTION_CLOSE: i32 = 0;
/// Open the kernel log (no-op).
pub const SYSLOG_ACTION_OPEN: i32 = 1;
/// Read up to `len` bytes from the log.
pub const SYSLOG_ACTION_READ: i32 = 2;
/// Read all remaining log messages.
pub const SYSLOG_ACTION_READ_ALL: i32 = 3;
/// Read all messages, then clear the ring buffer.
pub const SYSLOG_ACTION_READ_CLEAR: i32 = 4;
/// Clear the ring buffer (no buf/len needed).
pub const SYSLOG_ACTION_CLEAR: i32 = 5;
/// Disable console printing of new messages.
pub const SYSLOG_ACTION_CONSOLE_OFF: i32 = 6;
/// Re-enable console printing.
pub const SYSLOG_ACTION_CONSOLE_ON: i32 = 7;
/// Set the console log level (passed in `len`, 1..=8).
pub const SYSLOG_ACTION_CONSOLE_LEVEL: i32 = 8;
/// Return the number of unread bytes in the log.
pub const SYSLOG_ACTION_SIZE_UNREAD: i32 = 9;
/// Return the size of the log buffer.
pub const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;

/// Highest defined klogctl command.  Anything above this is `EINVAL`
/// per Linux `kernel/printk/printk.c::do_syslog`.
pub const SYSLOG_ACTION_MAX: i32 = SYSLOG_ACTION_SIZE_BUFFER;

/// Lowest console log level accepted by `SYSLOG_ACTION_CONSOLE_LEVEL`
/// (matches Linux `MINIMUM_CONSOLE_LOGLEVEL`).
pub const SYSLOG_LOG_LEVEL_MIN: i32 = 1;
/// Highest console log level accepted (matches Linux `LOGLEVEL_DEBUG + 1`).
pub const SYSLOG_LOG_LEVEL_MAX: i32 = 8;

/// Control the kernel log.
///
/// Stub: validates arguments per Linux `kernel/printk/printk.c::do_syslog`,
/// then returns `-1` with `ENOSYS`.  Our OS uses structured text logging
/// (JSON-lines per `design.txt`), not the legacy klog ring buffer.
///
/// # Linux semantics
///
/// `do_syslog` performs its argument checks in this order (after the
/// permission check, which we don't model here):
///
/// 1. The cmd selector enters a `switch (type)`.  An unrecognised cmd
///    hits the `default:` arm and returns `-EINVAL`.
/// 2. For `SYSLOG_ACTION_READ`, `_READ_ALL`, `_READ_CLEAR`, Linux folds
///    the buf and len checks into a single test:
///       `error = -EINVAL; if (!buf || len < 0) goto out;`
///    so a NULL `buf` returns **EINVAL**, *not* EFAULT.  EFAULT only
///    appears later from `!access_ok(buf, len)`, which we cannot model
///    in a stub.
/// 3. **Phase 157:** Linux follows the EINVAL check with
///    `if (!len) return 0;` — a zero-byte read is a no-op success that
///    is reported **before** any backend or access_ok dispatch.  We
///    mirror this so callers asking for zero bytes get `0`, not ENOSYS.
/// 4. For `SYSLOG_ACTION_CONSOLE_LEVEL`, Linux rejects `len < 1 || len > 8`
///    with EINVAL.
///
/// Errors (Linux-matching priority order):
/// * `EINVAL` — `cmd` is negative or above `SYSLOG_ACTION_MAX` (10).
/// * `EINVAL` — read commands (READ, READ_ALL, READ_CLEAR) with NULL
///   `buf` **or** `len < 0`.  Linux returns EINVAL (not EFAULT) for
///   NULL buf in the read family.
/// * `0`     — read commands with valid `buf` and `len == 0` (Phase 157).
/// * `EINVAL` — `SYSLOG_ACTION_CONSOLE_LEVEL` with `len` outside
///   `[1, 8]`.
/// * `EPERM` — **Phase 172:** Linux `check_syslog_permissions` rejects
///   the action when the caller lacks `CAP_SYSLOG`.  The CLOSE, OPEN,
///   and SIZE_BUFFER actions are unconditionally allowed (they are
///   no-ops on Linux).  Under default `dmesg_restrict=0`, READ_ALL and
///   SIZE_UNREAD are also unprivileged-accessible (they only read the
///   ring buffer for the calling process's view).  Every other action
///   — READ, READ_CLEAR, CLEAR, CONSOLE_OFF, CONSOLE_ON,
///   CONSOLE_LEVEL — requires CAP_SYSLOG; missing the cap returns
///   `EPERM` *after* argument-domain EINVAL checks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn klogctl(cmd: i32, buf: *mut u8, len: i32) -> i32 {
    if !(0..=SYSLOG_ACTION_MAX).contains(&cmd) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    match cmd {
        SYSLOG_ACTION_READ | SYSLOG_ACTION_READ_ALL | SYSLOG_ACTION_READ_CLEAR => {
            // Linux folds these two checks into a single EINVAL:
            //     error = -EINVAL;
            //     if (!buf || len < 0) goto out;
            // NULL buf is *not* EFAULT here; EFAULT only fires from
            // access_ok() further down, which we don't model.
            if buf.is_null() || len < 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // Phase 157: Linux do_syslog has `if (!len) return 0;` right
            // after the EINVAL guard.  A zero-byte read is a guaranteed
            // no-op success regardless of backend state.  Without this
            // we'd return ENOSYS for `klogctl(READ, valid_buf, 0)`,
            // diverging from glibc/musl-tested expectations.
            if len == 0 {
                return 0;
            }
        }
        SYSLOG_ACTION_CONSOLE_LEVEL => {
            if !(SYSLOG_LOG_LEVEL_MIN..=SYSLOG_LOG_LEVEL_MAX).contains(&len) {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
        }
        _ => {
            // CLOSE, OPEN, CLEAR, CONSOLE_OFF, CONSOLE_ON,
            // SIZE_UNREAD, SIZE_BUFFER — no buf/len validation needed.
        }
    }
    // Phase 172: Linux `check_syslog_permissions` gates most actions on
    // CAP_SYSLOG.  Mirror the default-`dmesg_restrict=0` semantics:
    //   • CLOSE/OPEN/SIZE_BUFFER  -> always allowed (no-op privileges)
    //   • READ_ALL/SIZE_UNREAD    -> allowed without cap (read-only ring view)
    //   • everything else         -> require CAP_SYSLOG -> EPERM
    // This check follows the argument-domain EINVAL guards so that
    // bad-cmd / NULL-buf / bad-CONSOLE_LEVEL still trump cap failure,
    // matching Linux's switch-then-permission ordering.
    let cap_required = !matches!(
        cmd,
        SYSLOG_ACTION_CLOSE
            | SYSLOG_ACTION_OPEN
            | SYSLOG_ACTION_SIZE_BUFFER
            | SYSLOG_ACTION_READ_ALL
            | SYSLOG_ACTION_SIZE_UNREAD,
    );
    if cap_required
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SYSLOG,
        )
    {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// glibc convenience: get_nprocs, get_nprocs_conf, etc.
// ---------------------------------------------------------------------------

/// `get_nprocs` — get number of available (online) processors.
///
/// glibc extension.  Equivalent to `sysconf(_SC_NPROCESSORS_ONLN)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn get_nprocs() -> i32 {
    sysconf(_SC_NPROCESSORS_ONLN) as i32
}

/// `get_nprocs_conf` — get number of configured processors.
///
/// glibc extension.  Equivalent to `sysconf(_SC_NPROCESSORS_CONF)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn get_nprocs_conf() -> i32 {
    sysconf(_SC_NPROCESSORS_CONF) as i32
}

/// `get_phys_pages` — get total number of physical pages.
///
/// glibc extension.  Equivalent to `sysconf(_SC_PHYS_PAGES)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn get_phys_pages() -> i64 {
    sysconf(_SC_PHYS_PAGES)
}

/// `get_avphys_pages` — get number of available physical pages.
///
/// glibc extension.  Equivalent to `sysconf(_SC_AVPHYS_PAGES)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn get_avphys_pages() -> i64 {
    sysconf(_SC_AVPHYS_PAGES)
}

// ---------------------------------------------------------------------------
// futimesat — change file timestamps relative to directory fd
// ---------------------------------------------------------------------------

/// `futimesat` — change file timestamps relative to a directory fd.
///
/// Superseded by `utimensat`, but some programs still use it.
/// If `dirfd` is `AT_FDCWD` or `path` is absolute, delegates to `utimes`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futimesat(
    dirfd: i32,
    path: *const u8,
    times: *const crate::file::Timeval,
) -> i32 {
    if dirfd == crate::file::AT_FDCWD || crate::file::is_absolute_path(path) {
        return crate::file::utimes(path, times);
    }
    let mut full = [0u8; PATH_MAX];
    let len = crate::file::resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    crate::file::utimes(full.as_ptr(), times)
}

// ---------------------------------------------------------------------------
// tmpnam_r — thread-safe tmpnam
// ---------------------------------------------------------------------------

/// `tmpnam_r` — generate a unique temporary filename (thread-safe).
///
/// Like `tmpnam`, but returns null if `s` is null (never uses an
/// internal static buffer).  Writes a unique name to `s` (which must
/// be at least `L_tmpnam` bytes).
///
/// Returns `s` on success, null on error.
///
/// # Safety
///
/// `s` must point to a buffer of at least `L_tmpnam` (20) bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn tmpnam_r(s: *mut u8) -> *mut u8 {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    // Delegate to stdio::tmpnam which populates the buffer.
    crate::stdio::tmpnam(s)
}

// ---------------------------------------------------------------------------
// get_current_dir_name — glibc extension
// ---------------------------------------------------------------------------

/// `get_current_dir_name` — get the current working directory.
///
/// Like `getcwd`, but allocates the buffer with `malloc`.
/// The caller must `free` the returned pointer.
///
/// glibc extension.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn get_current_dir_name() -> *mut u8 {
    let mut buf = [0u8; PATH_MAX];
    let ret = getcwd(buf.as_mut_ptr(), PATH_MAX);
    if ret.is_null() {
        return core::ptr::null_mut();
    }

    // Find length.
    let mut len: usize = 0;
    while len < PATH_MAX && buf[len] != 0 {
        len = len.wrapping_add(1);
    }

    // Allocate and copy.
    let alloc_size = len.wrapping_add(1); // Include null terminator.
    let ptr = crate::malloc::malloc(alloc_size);
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: ptr is valid for alloc_size bytes.
    unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, alloc_size); }
    ptr
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
    fn test_sysconf_nprocessors_positive() {
        assert!(sysconf(_SC_NPROCESSORS_ONLN) >= 1);
        assert!(sysconf(_SC_NPROCESSORS_CONF) >= 1);
    }

    #[test]
    fn test_sysconf_nprocessors_conf_ge_onln() {
        // Configured CPUs should be >= online CPUs.  We don't model offline
        // CPUs, so they're equal.
        assert_eq!(
            sysconf(_SC_NPROCESSORS_CONF),
            sysconf(_SC_NPROCESSORS_ONLN),
        );
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
        // fpathconf should return the same values as pathconf — once
        // the fd validator passes.  Allocate a real fd so we don't
        // depend on fd 0 being open.
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0)
            .expect("alloc_fd File failed");
        assert_eq!(fpathconf(fd, _PC_PATH_MAX), pathconf(core::ptr::null(), _PC_PATH_MAX));
        assert_eq!(fpathconf(fd, _PC_NAME_MAX), pathconf(core::ptr::null(), _PC_NAME_MAX));
        let _ = crate::fdtable::close_fd(fd);
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
    fn test_prctl_set_name_with_buffer_succeeds() {
        // Post-Phase-76 PR_SET_NAME requires a non-NULL buffer; with
        // one, the call succeeds (we accept and discard).
        let name = b"hello\0";
        assert_eq!(prctl(PR_SET_NAME, name.as_ptr() as u64, 0, 0, 0), 0);
    }

    #[test]
    fn test_prctl_set_no_new_privs_succeeds() {
        // Phase 160: bit is now persisted globally.  Reset around the
        // call so we don't leak state to other tests.
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
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
    // Phase 76 — prctl argument-domain validation
    // ------------------------------------------------------------------

    #[test]
    fn test_phase76_prctl_set_name_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NAME, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_phase76_prctl_get_name_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_GET_NAME, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_phase76_prctl_get_name_valid_buf_zeroes_first_byte() {
        // Phase 159 update: PR_GET_NAME now writes the full
        // TASK_COMM_LEN (16) bytes — matches Linux's
        // `copy_to_user(arg2, comm, sizeof(comm))`.  Pre-Phase-159 we
        // only wrote `buf[0]`, leaving `buf[1..16]` containing caller
        // stack.  The old assertion `buf[1] == 'X'` is now wrong and
        // has been replaced with a full-buffer zero check.  Sentinel
        // assertion `buf[1] != 'X'` is in
        // `test_prctl_get_name_no_longer_leaves_tail_uninitialised_phase159`.
        let mut buf = [b'X'; 16];
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        assert_eq!(buf[0], 0);
        // Phase 159: rest of buffer also zeroed.
        for (i, &b) in buf.iter().enumerate() {
            assert_eq!(b, 0, "PR_GET_NAME must zero byte {i} (Phase 159)");
        }
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_arg2_zero_einval() {
        // arg2 must be 1 — not 0.
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_arg2_two_einval() {
        // arg2 == 2 is not a valid boolean.
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 2, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_arg2_max_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, u64::MAX, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_arg3_nonzero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_arg4_nonzero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 99, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_arg5_nonzero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, u64::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_all_extra_args_max_einval() {
        // Garbage in every extra slot — must still be EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(
            prctl(PR_SET_NO_NEW_PRIVS, 1, u64::MAX, u64::MAX, u64::MAX),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_get_no_new_privs_arg2_nonzero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 1, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_get_no_new_privs_arg3_nonzero_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_get_no_new_privs_all_zero_ok() {
        // Phase 160: ensure the bit starts cleared so we assert the
        // "fresh process" value (0).
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 0);
        // Success path must not stamp errno.
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_phase76_prctl_set_no_new_privs_correct_call_ok() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(crate::errno::get_errno(), 0);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn test_phase76_prctl_unknown_option_still_einval() {
        // Regression: changing the dispatch must not break the
        // catch-all path.
        crate::errno::set_errno(0);
        assert_eq!(prctl(9_999, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_negative_option_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(-1, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_int_min_option_einval() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(i32::MIN, u64::MAX, u64::MAX, u64::MAX, u64::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Ordering: NULL-buf check beats unrelated extra-arg checks --

    #[test]
    fn test_phase76_prctl_set_name_null_beats_extra_args() {
        // PR_SET_NAME doesn't validate arg3..5 (matches Linux's lax
        // policy for pre-2.6 opcodes), so EFAULT must still surface
        // regardless of what's in the trailing slots.
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NAME, 0, u64::MAX, u64::MAX, u64::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_phase76_prctl_get_name_null_beats_extra_args() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_GET_NAME, 0, 1, 2, 3), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- Buggy-caller patterns --

    #[test]
    fn test_phase76_prctl_buggy_caller_uses_pthread_setname_style() {
        // glibc's pthread_setname_np internally does
        //   prctl(PR_SET_NAME, name).  A buggy caller forgets to set
        // arg2 and passes 0 — must surface EFAULT, not silently succeed.
        crate::errno::set_errno(0);
        let ret = prctl(PR_SET_NAME, 0, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_phase76_prctl_buggy_caller_set_nnp_with_zero() {
        // Caller mis-remembers PR_SET_NO_NEW_PRIVS and passes 0
        // thinking "0 = enable".  Must reject — only 1 is accepted.
        crate::errno::set_errno(0);
        let ret = prctl(PR_SET_NO_NEW_PRIVS, 0, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase76_prctl_workflow_roundtrip_get_after_set() {
        // Phase 160 retask: Pre-Phase-160 the stub didn't flip the bit,
        // so GET returned 0 even after a successful SET.  Post-fix the
        // bit is persisted, so GET returns 1.  The sentinel for the old
        // behaviour lives in
        // `test_prctl_get_after_set_no_longer_returns_zero_phase160`.
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1);
        assert_eq!(crate::errno::get_errno(), 0);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    // ------------------------------------------------------------------
    // Phase 159 — PR_GET_NAME writes the full TASK_COMM_LEN buffer
    //
    // Linux `kernel/sys.c::sys_prctl` for `PR_GET_NAME` does:
    //
    //     get_task_comm(comm, me);
    //     if (copy_to_user((char __user *)arg2, comm, sizeof(comm)))
    //         return -EFAULT;
    //
    // where `sizeof(comm) == TASK_COMM_LEN == 16`.  Every byte in the
    // user buffer is written (the name is NUL-padded to 16 bytes inside
    // `task_struct->comm`).
    //
    // Pre-Phase-159 our stub only wrote `arg2[0] = 0`, leaving
    // `arg2[1..16]` containing whatever uninitialised stack the caller
    // passed in.  Callers like glibc's `pthread_getname_np` copy the
    // full 16-byte region into their own storage and would observe
    // leaked caller stack.
    // ------------------------------------------------------------------

    // -- Per-error-class (precedence vs Phase 76) -----------------------

    /// EFAULT-on-NULL still wins over the 16-byte write — verify the
    /// fault path didn't accidentally start writing to NULL.
    #[test]
    fn test_phase159_get_name_null_buf_still_efault() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_GET_NAME, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- Full-buffer-write checks --------------------------------------

    /// Core Phase-159 contract: every one of the 16 bytes is zeroed.
    /// Pre-fix only `buf[0]` would be 0, the rest would stay 'X'.
    #[test]
    fn test_phase159_get_name_writes_all_16_bytes() {
        let mut buf = [b'X'; TASK_COMM_LEN];
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        assert_eq!(buf, [0u8; TASK_COMM_LEN]);
    }

    /// Sentinel-after-buffer check: the byte *immediately past* the
    /// 16-byte buffer must NOT be written.  Confirms we copy exactly
    /// TASK_COMM_LEN bytes — no overrun.
    #[test]
    fn test_phase159_get_name_does_not_overrun_past_16_bytes() {
        // Layout: [name buf | sentinel].
        let mut storage: [u8; TASK_COMM_LEN + 1] = [b'S'; TASK_COMM_LEN + 1];
        let ret = prctl(PR_GET_NAME, storage.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        // First 16 bytes zeroed.
        for i in 0..TASK_COMM_LEN {
            assert_eq!(storage[i], 0, "byte {i} should be zeroed");
        }
        // Sentinel byte at index 16 must remain.
        assert_eq!(storage[TASK_COMM_LEN], b'S', "no overrun past TASK_COMM_LEN");
    }

    /// Buffer pre-populated with the *previous* name (caller is reusing
    /// the buffer) must be fully overwritten, not partially leaked.
    #[test]
    fn test_phase159_get_name_overwrites_stale_name() {
        let mut buf = *b"OLDNAMEHERELEAK\0"; // 16 bytes
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        assert_eq!(buf, [0u8; TASK_COMM_LEN], "stale name must be fully wiped");
    }

    /// Buffer pre-populated with high-entropy junk (closest analogue to
    /// uninitialised stack) — the post-Phase-159 contract is that no
    /// byte of the junk survives.
    #[test]
    fn test_phase159_get_name_clears_high_entropy_junk() {
        let pattern: [u8; TASK_COMM_LEN] = [
            0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE,
            0xFE, 0xED, 0xFA, 0xCE, 0xA5, 0x5A, 0x12, 0x34,
        ];
        let mut buf = pattern;
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        for (i, &b) in buf.iter().enumerate() {
            assert_eq!(b, 0, "byte {i} stayed 0x{:02X} from {:02X?}", pattern[i], pattern);
        }
    }

    // -- Workflow: pthread_getname_np-style readback --------------------

    /// glibc's `pthread_getname_np` does:
    ///   prctl(PR_GET_NAME, buf, 0, 0, 0);
    ///   strlen(buf); /* expects NUL terminator within 16 bytes */
    /// The post-Phase-159 contract guarantees a NUL within the first
    /// 16 bytes (in fact at index 0 — we don't track a name).
    #[test]
    fn test_phase159_workflow_pthread_getname_style() {
        let mut buf = [0xAAu8; TASK_COMM_LEN];
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        // strlen-style scan: a NUL must appear within TASK_COMM_LEN.
        let nul_pos = buf.iter().position(|&b| b == 0);
        assert!(nul_pos.is_some(), "no NUL terminator within 16 bytes");
        // No junk between buffer start and the (first) NUL.
        assert_eq!(nul_pos, Some(0));
    }

    // -- Extra-arg liberties (Linux ignores arg3..5 for these opcodes) --

    /// Linux's `PR_GET_NAME` ignores arg3..5; Phase 159 keeps that
    /// lenient behaviour even though we now write 16 bytes.
    #[test]
    fn test_phase159_get_name_extra_args_ignored() {
        let mut buf = [b'X'; TASK_COMM_LEN];
        let ret = prctl(
            PR_GET_NAME,
            buf.as_mut_ptr() as u64,
            u64::MAX,
            u64::MAX,
            u64::MAX,
        );
        assert_eq!(ret, 0);
        assert_eq!(buf, [0u8; TASK_COMM_LEN]);
    }

    // -- Buggy-caller patterns -----------------------------------------

    /// Caller passes a buffer that is partially overlapping with their
    /// own stack frame.  As long as the 16-byte region is writable, the
    /// write succeeds — and only those 16 bytes are touched.
    #[test]
    fn test_phase159_get_name_buggy_caller_short_storage_ok() {
        // Slightly oversized storage; we slice the buffer down to 16.
        let mut storage = [b'Z'; 24];
        let ret = prctl(PR_GET_NAME, storage.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        // First 16 zeroed.
        for i in 0..TASK_COMM_LEN {
            assert_eq!(storage[i], 0);
        }
        // Bytes 16..24 untouched.
        for i in TASK_COMM_LEN..24 {
            assert_eq!(storage[i], b'Z', "byte {i} should be untouched");
        }
    }

    // -- Recovery / no-side-effect loop --------------------------------

    /// 200 iterations of PR_GET_NAME on a fresh buffer must all succeed
    /// without errno desync or partial writes.
    #[test]
    fn test_phase159_get_name_repeated_calls_idempotent() {
        for i in 0..200 {
            let mut buf = [(i as u8).wrapping_add(1); TASK_COMM_LEN];
            crate::errno::set_errno(0);
            let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
            assert_eq!(ret, 0, "iter {i}");
            assert_eq!(buf, [0u8; TASK_COMM_LEN], "iter {i} buf mismatch");
            assert_eq!(crate::errno::get_errno(), 0, "iter {i} errno");
        }
    }

    /// Failed call (NULL) must not write anything — and the next
    /// successful call must still produce a fully-zeroed buffer.
    #[test]
    fn test_phase159_get_name_recovery_after_efault() {
        // 1. NULL → EFAULT, no writes.
        assert_eq!(prctl(PR_GET_NAME, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        // 2. Valid call immediately after still works.
        let mut buf = [b'Y'; TASK_COMM_LEN];
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        assert_eq!(buf, [0u8; TASK_COMM_LEN]);
    }

    // -- Sentinel: pre-Phase-159 behaviour no longer holds -------------

    /// Sentinel: the pre-Phase-159 contract was "PR_GET_NAME only writes
    /// `buf[0]`."  This test pins the post-fix contract — `buf[1]` is
    /// also written (and equals 0).  If anyone reverts to the partial
    /// write this fails immediately.
    #[test]
    fn test_prctl_get_name_no_longer_leaves_tail_uninitialised_phase159() {
        let mut buf = [b'X'; TASK_COMM_LEN];
        let _ = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        // Pre-Phase-159 buf[15] stayed 'X'.  Post-fix it's 0.
        assert_ne!(buf[15], b'X');
        assert_eq!(buf[15], 0);
    }

    // -- Cross-checks: TASK_COMM_LEN constant and PR_SET_NAME unchanged --

    /// TASK_COMM_LEN must remain 16 — this is a Linux ABI constant and
    /// changing it would silently break every caller.
    #[test]
    fn test_phase159_task_comm_len_is_16() {
        assert_eq!(TASK_COMM_LEN, 16);
    }

    /// Cross-check: `PR_SET_NAME` semantics unchanged by Phase 159.
    /// Phase 159 only touches the GET path.
    #[test]
    fn test_phase159_set_name_unaffected() {
        let name = b"hello\0world\0xxxx";
        assert_eq!(prctl(PR_SET_NAME, name.as_ptr() as u64, 0, 0, 0), 0);
    }

    /// Cross-check: NULL-PR_SET_NAME still EFAULT.
    #[test]
    fn test_phase159_set_name_null_still_efault() {
        crate::errno::set_errno(0);
        assert_eq!(prctl(PR_SET_NAME, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ------------------------------------------------------------------
    // Phase 160 — PR_SET_NO_NEW_PRIVS / PR_GET_NO_NEW_PRIVS round-trip
    //
    // Linux `kernel/sys.c::sys_prctl`:
    //
    //   case PR_SET_NO_NEW_PRIVS:
    //       if (arg2 != 1 || arg3 || arg4 || arg5) return -EINVAL;
    //       task_set_no_new_privs(current);
    //       break;
    //
    //   case PR_GET_NO_NEW_PRIVS:
    //       if (arg2 || arg3 || arg4 || arg5) return -EINVAL;
    //       return task_no_new_privs(current) ? 1 : 0;
    //
    // The bit is **one-way**: once set, it cannot be cleared except by
    // a fresh execve (which our single-process model doesn't have, so
    // effectively never).  Pre-Phase-160 our stub silently discarded
    // the SET and always returned 0 from GET — breaking sandbox callers
    // (Chromium, Docker `--security-opt=no-new-privileges`, Bubblewrap).
    //
    // Tests run with --test-threads=1 and the bit is a process-global
    // atomic, so each test that touches it explicitly stores `false`
    // at start (and at end, if it sets) to keep test order irrelevant.
    // ------------------------------------------------------------------

    // -- Per-error-class: precedence unchanged ---------------------------

    /// Sanity: fresh bit reads 0.  This guards against accumulated
    /// state when running the file under cargo's alphabetic test order.
    #[test]
    fn test_phase160_fresh_bit_reads_zero() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 0);
    }

    /// Sanity: SET arg-validation precedence preserved — EINVAL on
    /// bad arg2 short-circuits BEFORE storing the bit.
    #[test]
    fn test_phase160_set_bad_arg2_does_not_store_bit() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        // arg2 == 2 → EINVAL.
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 2, 0, 0, 0), -1);
        // Bit is still cleared.
        assert!(!no_new_privs_set());
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 0);
    }

    /// SET with bad arg3 likewise must not store the bit.
    #[test]
    fn test_phase160_set_bad_arg3_does_not_store_bit() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 99, 0, 0), -1);
        assert!(!no_new_privs_set());
    }

    // -- Round-trip core --------------------------------------------------

    /// Core Phase-160 fix: SET then GET returns 1.
    #[test]
    fn test_phase160_set_then_get_returns_one() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    /// One-way invariant: once set, repeated SET still leaves the bit
    /// set (idempotent), and there is no way to clear it via prctl.
    /// Linux's contract: only `execve` can clear it.
    #[test]
    fn test_phase160_one_way_invariant() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1);
        // Second SET: still succeeds, bit still 1.
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1);
        // The clearing arg (arg2 == 0) is rejected with EINVAL — so
        // there's no API path to flip the bit back.
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    // -- no_new_privs_set() accessor --------------------------------------

    /// Public accessor reflects the bit after SET — other parts of the
    /// posix layer (seccomp, capset, future capability-raising stubs)
    /// will gate on this.
    #[test]
    fn test_phase160_accessor_after_set() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert!(!no_new_privs_set());
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert!(no_new_privs_set());
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert!(!no_new_privs_set());
    }

    // -- Workflow: chromium-style sandbox init ---------------------------

    /// Chromium's sandbox does:
    ///   1. Drop capabilities.
    ///   2. prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0).
    ///   3. Verify with prctl(PR_GET_NO_NEW_PRIVS) — must return 1.
    /// If step 3 returns 0, the sandbox aborts because it can't trust
    /// that subsequent exec calls won't gain new privileges.
    #[test]
    fn test_phase160_workflow_chromium_sandbox_verify() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        // (steps 1-2 fused — we don't have a cap-drop step here)
        let set_ret = prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        assert_eq!(set_ret, 0);
        // Step 3: verification readback.
        let get_ret = prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0);
        assert_eq!(get_ret, 1, "sandbox verification readback must see 1");
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    /// Bubblewrap-style: SET twice (defensive double-set), then GET.
    /// Must still return 1, not 2 or some accumulated counter.
    #[test]
    fn test_phase160_workflow_double_set_then_get_returns_one() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    // -- Buggy-caller patterns -------------------------------------------

    /// Caller passes garbage arg2 thinking "non-zero = enable."  Must
    /// reject and NOT set the bit.  Subsequent GET still reads 0.
    #[test]
    fn test_phase160_buggy_caller_garbage_arg2_no_state_mutation() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        let ret = prctl(PR_SET_NO_NEW_PRIVS, 0xDEAD_BEEF, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // Bit untouched.
        assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 0);
    }

    /// Caller passes valid arg2=1 but garbage in arg3 — also rejected,
    /// bit also untouched.
    #[test]
    fn test_phase160_buggy_caller_extra_arg_does_not_set_bit() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        let ret = prctl(PR_SET_NO_NEW_PRIVS, 1, 1, 0, 0);
        assert_eq!(ret, -1);
        // Bit untouched even though arg2 was valid.
        assert!(!no_new_privs_set());
    }

    // -- Recovery / no-side-effect loop ----------------------------------

    /// 200 SET calls after a clean start — bit ends up set, all SETs
    /// succeed, no errno desync.
    #[test]
    fn test_phase160_repeated_set_idempotent_loop() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        for i in 0..200 {
            crate::errno::set_errno(0);
            let ret = prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
            assert_eq!(ret, 0, "iter {i} SET failed");
            assert_eq!(crate::errno::get_errno(), 0, "iter {i} errno changed");
            assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 1, "iter {i} GET");
        }
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    /// 200 GET calls with no SET — bit stays cleared, all GETs return 0.
    #[test]
    fn test_phase160_repeated_get_no_set_returns_zero() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        for i in 0..200 {
            assert_eq!(prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0), 0, "iter {i}");
        }
    }

    // -- Sentinel: pre-Phase-160 behaviour no longer holds ---------------

    /// Sentinel: the pre-Phase-160 contract was "GET always returns 0
    /// even after a successful SET."  Asserting the opposite here pins
    /// the new contract in place.
    #[test]
    fn test_prctl_get_after_set_no_longer_returns_zero_phase160() {
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
        assert_eq!(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        let got = prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0);
        // Pre-fix: 0.  Post-fix: 1.
        assert_ne!(got, 0);
        assert_eq!(got, 1);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    // -- Cross-checks: other prctl options unaffected -------------------

    /// Cross-check: PR_GET_NAME unaffected by Phase 160.
    #[test]
    fn test_phase160_get_name_unaffected() {
        NO_NEW_PRIVS.store(true, core::sync::atomic::Ordering::Relaxed);
        let mut buf = [b'X'; TASK_COMM_LEN];
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        assert_eq!(buf, [0u8; TASK_COMM_LEN]);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    /// Cross-check: unknown prctl options still EINVAL regardless of
    /// the no_new_privs bit state.
    #[test]
    fn test_phase160_unknown_prctl_still_einval_when_bit_set() {
        NO_NEW_PRIVS.store(true, core::sync::atomic::Ordering::Relaxed);
        crate::errno::set_errno(0);
        assert_eq!(prctl(9_999, 0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        NO_NEW_PRIVS.store(false, core::sync::atomic::Ordering::Relaxed);
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
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0)
            .expect("alloc_fd File failed");
        assert_eq!(syncfs(fd), 0);
        let _ = crate::fdtable::close_fd(fd);
    }

    // -- Phase 72: syncfs validator --

    #[test]
    fn test_syncfs_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(syncfs(-1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_syncfs_unopen_fd_ebadf() {
        let probe: i32 = 0x4000_0051;
        if crate::fdtable::get_fd(probe).is_some() {
            let _ = crate::fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(syncfs(probe), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_syncfs_min_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(syncfs(i32::MIN), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_syncfs_pipe_fd_returns_zero() {
        // syncfs on a pipe is permitted on Linux (it walks to the
        // pipe's superblock); we accept any open fd.
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::Pipe, 1)
            .expect("alloc_fd Pipe failed");
        assert_eq!(syncfs(fd), 0);
        let _ = crate::fdtable::close_fd(fd);
    }

    #[test]
    fn test_buggy_caller_syncfs_stale_fd() {
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0)
            .expect("alloc_fd File failed");
        let _ = crate::fdtable::close_fd(fd);
        crate::errno::set_errno(0);
        assert_eq!(syncfs(fd), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
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
    // Phase 167: sethostname / setdomainname — CAP_SYS_ADMIN gate
    //
    // Linux's `kernel/sys.c::sys_sethostname` and `sys_setdomainname`
    // check `ns_capable(uts_ns->user_ns, CAP_SYS_ADMIN)` at the very
    // top of the syscall prologue — before length validation and
    // before the user-pointer is touched.  Pre-Phase-167 our stubs
    // accepted writes from any caller; the setdomainname doc even
    // said "we're single-user so any process can set the domain
    // name."  Now that the cred model exists (Phase 77 reboot, 164
    // swap, 165 mount, 166 chroot), we restore Linux's gate.
    //
    // Ordering: EPERM > EINVAL (len > HOST_NAME_MAX) > EFAULT
    // (NULL pointer with len > 0).  `len == 0` short-circuits the
    // copy and is accepted with any pointer (matches
    // copy_from_user(_, NULL, 0) returning 0).
    // ------------------------------------------------------------------

    mod uts_cap_phase167 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 77 / 164 / 165 / 166.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_admin() {
            use crate::sys_capability::CAP_SYS_ADMIN;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_ADMIN < 32 {
                (lo & !(1u32 << CAP_SYS_ADMIN), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_ADMIN - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_ADMIN");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_ADMIN));
        }

        // -- Per-error-class --------------------------------------------------

        /// `sethostname` under no cap returns EPERM, not the silent
        /// success the stub used to grant.
        #[test]
        fn test_sethostname_phase167_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let name = b"newhost";
            assert_eq!(sethostname(name.as_ptr(), name.len()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Same for `setdomainname`.
        #[test]
        fn test_setdomainname_phase167_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let name = b"example.com";
            assert_eq!(setdomainname(name.as_ptr(), name.len()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix --------------------------------------------------

        /// EPERM must beat the EINVAL-too-long check (cap goes
        /// first in Linux's prologue).
        #[test]
        fn test_sethostname_phase167_eperm_beats_einval_too_long() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let long = [b'a'; 1024];
            assert_eq!(sethostname(long.as_ptr(), 1024), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// EPERM must beat EFAULT (NULL pointer + len > 0).
        #[test]
        fn test_sethostname_phase167_eperm_beats_efault_null_with_len() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(sethostname(core::ptr::null(), 5), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// setdomainname: EPERM beats EINVAL.
        #[test]
        fn test_setdomainname_phase167_eperm_beats_einval_too_long() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let long = [b'b'; 1024];
            assert_eq!(setdomainname(long.as_ptr(), 1024), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// setdomainname: EPERM beats EFAULT.
        #[test]
        fn test_setdomainname_phase167_eperm_beats_efault_null_with_len() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(setdomainname(core::ptr::null(), 5), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Regression: under default caps, EINVAL still beats EFAULT
        /// — the new ordering only inserts EPERM at the top.
        #[test]
        fn test_sethostname_phase167_default_einval_beats_efault() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            assert_eq!(
                sethostname(core::ptr::null(), 100_000),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- Linux len=0 short-circuit (new in Phase 167) ---------------------

        /// `sethostname(NULL, 0)` succeeds on Linux because
        /// `copy_from_user(_, NULL, 0)` is a no-op.  Phase 167
        /// restores that semantics — pre-Phase-167 we returned
        /// EFAULT here, diverging from Linux.
        #[test]
        fn test_sethostname_phase167_null_len_zero_succeeds_with_cap() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            assert_eq!(sethostname(core::ptr::null(), 0), 0);
            // Re-set to localhost to avoid bleeding into later tests.
            let localhost = b"localhost";
            let _ = sethostname(localhost.as_ptr(), localhost.len());
        }

        /// Same for `setdomainname(NULL, 0)`.
        #[test]
        fn test_setdomainname_phase167_null_len_zero_succeeds_with_cap() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            assert_eq!(setdomainname(core::ptr::null(), 0), 0);
            // Restore "(none)" to avoid bleeding into later tests.
            let none = b"(none)";
            let _ = setdomainname(none.as_ptr(), none.len());
        }

        /// `sethostname(NULL, 0)` under no cap still returns EPERM
        /// — the cap check beats even the len=0 short-circuit.
        #[test]
        fn test_sethostname_phase167_null_len_zero_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(sethostname(core::ptr::null(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Workflow ---------------------------------------------------------

        /// Workflow: a dropbear sshd that already dropped
        /// CAP_SYS_ADMIN tries to "normalise" its hostname for
        /// logging — Linux returns EPERM; we now do too.
        #[test]
        fn test_sethostname_phase167_workflow_dropbear_post_drop() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let name = b"sshd-worker-7";
            assert_eq!(sethostname(name.as_ptr(), name.len()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Workflow: a Kubernetes pod (no CAP_SYS_ADMIN) calls
        /// `domainname svc.cluster.local` from an entrypoint script
        /// — must see EPERM.
        #[test]
        fn test_setdomainname_phase167_workflow_k8s_pod_entrypoint() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let domain = b"svc.cluster.local";
            assert_eq!(setdomainname(domain.as_ptr(), domain.len()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy-caller -----------------------------------------------------

        /// Buggy caller: a build script computes a hostname length
        /// from `strlen` on a stack-garbage pointer, ending up with
        /// `len = 0xFFFF` and an arbitrary `name`.  Under no cap we
        /// still get EPERM, not the EINVAL the unprivileged-callable
        /// path would have returned.
        #[test]
        fn test_sethostname_phase167_buggy_huge_len_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let stale = b"stale\0";
            assert_eq!(sethostname(stale.as_ptr(), 0xFFFF), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery ---------------------------------------------------------

        /// After EPERM, restoring CAP_SYS_ADMIN lets the next
        /// sethostname succeed.  Verifies the cap-restoration path
        /// is intact even after repeated cap-stripped calls.
        #[test]
        fn test_sethostname_phase167_recovery_after_eperm() {
            // Save the original hostname for restoration.
            let mut orig = [0u8; 256];
            gethostname(orig.as_mut_ptr(), orig.len());
            let orig_len = unsafe { crate::string::strlen(orig.as_ptr()) };

            {
                let _g = CapGuard::snapshot();
                drop_cap_sys_admin();
                errno::set_errno(0);
                assert_eq!(sethostname(b"test\0".as_ptr(), 4), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            } // guard drops here -> cap restored

            // With caps back, sethostname succeeds.
            let new_name = b"phase167-recover";
            assert_eq!(
                sethostname(new_name.as_ptr(), new_name.len()),
                0
            );

            // Restore the original hostname so later tests see the
            // baseline.
            sethostname(orig.as_ptr(), orig_len);
        }

        // -- Sentinels --------------------------------------------------------

        /// Pre-Phase-167 sethostname silently succeeded for any
        /// caller — this sentinel locks the new EPERM contract.
        #[test]
        fn test_sethostname_phase167_no_longer_silently_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let rc = sethostname(b"x\0".as_ptr(), 1);
            assert_ne!(rc, 0,
                "Pre-Phase-167: unprivileged sethostname returned 0 \
                 — CAP_SYS_ADMIN gate missing.");
            assert_eq!(rc, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Sentinel for setdomainname.
        #[test]
        fn test_setdomainname_phase167_no_longer_silently_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let rc = setdomainname(b"x\0".as_ptr(), 1);
            assert_ne!(rc, 0,
                "Pre-Phase-167: unprivileged setdomainname returned 0 \
                 — CAP_SYS_ADMIN gate missing.");
            assert_eq!(rc, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Cross-checks -----------------------------------------------------

        /// Both functions share the same EPERM precedence under a
        /// stripped cap.
        #[test]
        fn test_sethostname_and_setdomainname_phase167_share_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();

            errno::set_errno(0);
            assert_eq!(sethostname(b"h\0".as_ptr(), 1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);

            errno::set_errno(0);
            assert_eq!(setdomainname(b"d\0".as_ptr(), 1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Regression: default-cap sethostname still round-trips
        /// (the existing test_sethostname_roundtrip exercises this
        /// without explicit cap check; we add an explicit assertion
        /// to lock it down).
        #[test]
        fn test_sethostname_phase167_default_cap_still_writes() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));

            // Save and restore.
            let mut orig = [0u8; 256];
            gethostname(orig.as_mut_ptr(), orig.len());
            let orig_len = unsafe { crate::string::strlen(orig.as_ptr()) };

            let new_name = b"phase167-default";
            assert_eq!(
                sethostname(new_name.as_ptr(), new_name.len()),
                0
            );
            let mut buf = [0u8; 256];
            assert_eq!(gethostname(buf.as_mut_ptr(), buf.len()), 0);
            assert_eq!(&buf[..new_name.len()], new_name);

            sethostname(orig.as_ptr(), orig_len);
        }
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

    // ------------------------------------------------------------------
    // realpath — null argument handling
    // ------------------------------------------------------------------

    #[test]
    fn test_realpath_null_path() {
        let mut buf = [0u8; PATH_MAX];
        let ret = realpath(core::ptr::null(), buf.as_mut_ptr());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_realpath_null_resolved() {
        // resolved_path NULL means "allocate for me" in POSIX; we can't
        // malloc (no_std), so this stays EINVAL — it's a mode we don't
        // support, not a bad address.
        let ret = realpath(b"/tmp\0".as_ptr(), core::ptr::null_mut());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_realpath_empty_path() {
        let mut buf = [0u8; PATH_MAX];
        let ret = realpath(b"\0".as_ptr(), buf.as_mut_ptr());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    // ------------------------------------------------------------------
    // canonicalize_file_name — null argument handling
    // ------------------------------------------------------------------

    #[test]
    fn test_canonicalize_file_name_null() {
        let ret = canonicalize_file_name(core::ptr::null());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_canonicalize_file_name_empty_path() {
        let ret = canonicalize_file_name(b"\0".as_ptr());
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    // ------------------------------------------------------------------
    // seteuid / setegid / setreuid / setregid — stub success tests
    // ------------------------------------------------------------------

    #[test]
    fn test_seteuid_succeeds() {
        assert_eq!(seteuid(0), 0);
        assert_eq!(seteuid(1000), 0);
        assert_eq!(seteuid(u32::MAX), 0);
    }

    #[test]
    fn test_setegid_succeeds() {
        assert_eq!(setegid(0), 0);
        assert_eq!(setegid(1000), 0);
        assert_eq!(setegid(u32::MAX), 0);
    }

    #[test]
    fn test_setreuid_succeeds() {
        assert_eq!(setreuid(0, 0), 0);
        assert_eq!(setreuid(1000, 2000), 0);
        assert_eq!(setreuid(u32::MAX, u32::MAX), 0);
    }

    #[test]
    fn test_setregid_succeeds() {
        assert_eq!(setregid(0, 0), 0);
        assert_eq!(setregid(1000, 2000), 0);
        assert_eq!(setregid(u32::MAX, u32::MAX), 0);
    }

    // ------------------------------------------------------------------
    // setgroups — stub success
    // ------------------------------------------------------------------

    #[test]
    fn test_setgroups_empty() {
        assert_eq!(setgroups(0, core::ptr::null()), 0);
    }

    #[test]
    fn test_setgroups_non_empty() {
        let groups: [GidT; 3] = [100, 200, 300];
        assert_eq!(setgroups(3, groups.as_ptr()), 0);
    }

    // ------------------------------------------------------------------
    // Phase 85 — setgroups argument-domain validation
    //
    // Linux semantics being validated:
    //   - size > 65536 → -1, EINVAL
    //   - size > 0 && list NULL → -1, EFAULT
    //   - size == 0 → 0 regardless of list
    //   - Well-formed call → 0 (single-user, no EPERM path)
    // ------------------------------------------------------------------

    #[test]
    fn test_setgroups_phase85_zero_size_null_list_ok() {
        errno::set_errno(0);
        assert_eq!(setgroups(0, core::ptr::null()), 0);
    }

    #[test]
    fn test_setgroups_phase85_zero_size_with_nonnull_list_ok() {
        // Linux: size==0 means "drop all", list pointer is ignored.
        let groups: [GidT; 1] = [42];
        errno::set_errno(0);
        assert_eq!(setgroups(0, groups.as_ptr()), 0);
    }

    #[test]
    fn test_setgroups_phase85_nonzero_size_null_list_efault() {
        errno::set_errno(0);
        let ret = setgroups(1, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_setgroups_phase85_large_nonzero_size_null_list_efault() {
        errno::set_errno(0);
        let ret = setgroups(1000, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_setgroups_phase85_size_one_above_max_einval() {
        // 65537 > 65536 → EINVAL.  The pointer is irrelevant because
        // size validation comes first.
        let groups: [GidT; 1] = [0];
        errno::set_errno(0);
        let ret = setgroups(65537, groups.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setgroups_phase85_einval_takes_precedence_over_efault() {
        // size > NGROUPS_MAX AND list NULL → EINVAL (size check first).
        errno::set_errno(0);
        let ret = setgroups(usize::MAX, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setgroups_phase85_size_at_max_ok() {
        // size == 65536 is the maximum permitted by Linux.  We can't
        // actually allocate a 65536-element array on the test stack,
        // but a non-NULL pointer is what the validator needs.
        let groups: [GidT; 1] = [0];
        errno::set_errno(0);
        let ret = setgroups(65536, groups.as_ptr());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_setgroups_phase85_size_one_below_max_ok() {
        let groups: [GidT; 1] = [0];
        errno::set_errno(0);
        let ret = setgroups(65535, groups.as_ptr());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_setgroups_phase85_typical_group_set_ok() {
        let groups: [GidT; 5] = [0, 10, 100, 1000, 65534];
        errno::set_errno(0);
        let ret = setgroups(5, groups.as_ptr());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_setgroups_phase85_drop_all_groups_idiom() {
        // The classic privilege-drop idiom: setgroups(0, NULL).
        errno::set_errno(0);
        let ret = setgroups(0, core::ptr::null());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_setgroups_phase85_einval_then_valid_call_progression() {
        // An EINVAL failure must not taint a subsequent valid call.
        errno::set_errno(0);
        assert_eq!(setgroups(usize::MAX, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        assert_eq!(setgroups(0, core::ptr::null()), 0);
    }

    #[test]
    fn test_setgroups_phase85_efault_then_valid_call_progression() {
        // An EFAULT failure must not taint a subsequent valid call.
        errno::set_errno(0);
        assert_eq!(setgroups(1, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);

        let groups: [GidT; 1] = [1];
        errno::set_errno(0);
        assert_eq!(setgroups(1, groups.as_ptr()), 0);
    }

    #[test]
    fn test_setgroups_phase85_repeated_valid_calls_stable() {
        // No hidden global state — repeated calls behave identically.
        let groups: [GidT; 2] = [50, 60];
        for _ in 0..4 {
            errno::set_errno(0);
            assert_eq!(setgroups(2, groups.as_ptr()), 0);
        }
    }

    #[test]
    fn test_setgroups_phase85_size_one_with_valid_list_ok() {
        let groups: [GidT; 1] = [12345];
        errno::set_errno(0);
        let ret = setgroups(1, groups.as_ptr());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_setgroups_phase85_size_just_above_max_einval() {
        // 65537 → EINVAL, and 65538 → EINVAL, so confirm the boundary
        // condition isn't off-by-one in either direction.
        let groups: [GidT; 1] = [0];
        for size in [65537usize, 65538, 100_000, 1_000_000] {
            errno::set_errno(0);
            let ret = setgroups(size, groups.as_ptr());
            assert_eq!(ret, -1, "size={} should fail", size);
            assert_eq!(errno::get_errno(), errno::EINVAL, "size={}", size);
        }
    }

    // ----------------------------------------------------------------------
    // Phase 187: setgroups — CAP_SETGID gate
    // ----------------------------------------------------------------------
    //
    // Linux's `kernel/groups.c::SYSCALL_DEFINE2(setgroups, ...)` opens
    // with:
    //
    //     if (!may_setgroups())
    //         return -EPERM;
    //     if ((unsigned)gidsetsize > NGROUPS_MAX)
    //         return -EINVAL;
    //     ... groups_alloc / groups_from_user ...
    //
    // `may_setgroups()` returns `ns_capable_setid(user_ns, CAP_SETGID)
    // && userns_may_setgroups(user_ns)`.  We have a single user
    // namespace so the second factor is always true; the gate
    // collapses to a pure `CAP_SETGID` probe.
    //
    // Cap check beats EINVAL beats EFAULT.  Host test build holds
    // CAP_SETGID by default (DEFAULT_CAPS_LOW = u32::MAX includes bit
    // 6), so all 14 pre-existing Phase 85 setgroups tests reach the
    // EINVAL / EFAULT / success paths unchanged.
    //
    // These tests must run with `--test-threads=1` because they
    // manipulate process-wide capability state.
    // ----------------------------------------------------------------------

    mod setgroups_cap_phase187 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 167 (`uts_cap_phase167`).
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_setgid() {
            use crate::sys_capability::CAP_SETGID;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SETGID < 32 {
                (lo & !(1u32 << CAP_SETGID), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SETGID - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SETGID");
            assert!(!crate::sys_capability::has_capability(CAP_SETGID));
        }

        // -- Per-error-class --------------------------------------------------

        /// `setgroups(0, NULL)` — the canonical drop-all-groups idiom
        /// — used to silently succeed even without CAP_SETGID.  Phase
        /// 187 makes it report EPERM, matching Linux's `may_setgroups`
        /// gate at the very top of the syscall handler.
        #[test]
        fn test_setgroups_phase187_no_cap_drop_all_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgroups(0, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Same gate fires for a small non-empty list.
        #[test]
        fn test_setgroups_phase187_no_cap_small_list_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            let groups: [GidT; 3] = [1, 2, 3];
            assert_eq!(setgroups(3, groups.as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// And for the boundary-sized maximum.
        #[test]
        fn test_setgroups_phase187_no_cap_at_max_size_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            let groups: [GidT; 1] = [0];
            assert_eq!(setgroups(65536, groups.as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix --------------------------------------------------

        /// Cap missing AND size > NGROUPS_MAX → cap wins (EPERM, not
        /// EINVAL).  Matches Linux's `may_setgroups` placement at the
        /// top of the syscall, before the size guard.
        #[test]
        fn test_setgroups_phase187_eperm_beats_einval_oversize() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            let groups: [GidT; 1] = [0];
            assert_eq!(setgroups(65537, groups.as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM,
                "EPERM must beat EINVAL when cap is missing");
        }

        /// Cap missing AND size > 0 with NULL list → cap wins (EPERM,
        /// not EFAULT).
        #[test]
        fn test_setgroups_phase187_eperm_beats_efault_null_list() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgroups(5, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM,
                "EPERM must beat EFAULT when cap is missing");
        }

        /// Cap missing AND size = usize::MAX AND list = NULL → cap
        /// wins over both EINVAL and EFAULT.
        #[test]
        fn test_setgroups_phase187_eperm_beats_einval_and_efault() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgroups(usize::MAX, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Workflow --------------------------------------------------------

        /// Drop cap → setgroups fails; restore cap → setgroups
        /// succeeds.  Mirrors the privilege-drop / privilege-regain
        /// pattern container runtimes use during user-namespace setup.
        #[test]
        fn test_setgroups_phase187_drop_then_restore_workflow() {
            let _g = CapGuard::snapshot();
            // 1. Cap held → succeed.
            errno::set_errno(0);
            assert_eq!(setgroups(0, core::ptr::null()), 0);
            // 2. Drop cap → fail with EPERM.
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgroups(0, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // 3. Restore via guard drop happens after the test — we
            //    verify the in-test restore path by manually replaying
            //    a capset to u32::MAX and re-checking.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(setgroups(0, core::ptr::null()), 0);
        }

        // -- Buggy-caller ----------------------------------------------------

        /// A caller that forgot to clear errno before calling sees a
        /// fresh EPERM, not whatever stale value was sitting there.
        #[test]
        fn test_setgroups_phase187_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(errno::ENOENT);
            assert_eq!(setgroups(2, [1, 2].as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM,
                "Stale ENOENT must be overwritten with EPERM");
        }

        /// A caller that passes obviously broken arguments still sees
        /// EPERM, not the argument errno — matches Linux ordering.
        #[test]
        fn test_setgroups_phase187_buggy_caller_garbage_args_still_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            // Wildly oversized + null pointer + ignore-the-rules
            // values: would normally trip EINVAL then EFAULT, but the
            // cap check beats both.
            assert_eq!(
                setgroups(usize::MAX - 1, core::ptr::null()),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery --------------------------------------------------------

        /// CapGuard drop restores the cap so a subsequent valid call
        /// in the same test process succeeds with errno cleared.
        #[test]
        fn test_setgroups_phase187_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_setgid();
                errno::set_errno(0);
                assert_eq!(setgroups(0, core::ptr::null()), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            } // _g dropped here; cap restored.
            // Errno is still EPERM (the function does not clear it on
            // success) but a fresh call succeeds again.
            errno::set_errno(0);
            assert_eq!(setgroups(0, core::ptr::null()), 0);
            // Success path leaves errno untouched (0 from our reset).
            assert_eq!(errno::get_errno(), 0);
        }

        // -- No-side-effect --------------------------------------------------

        /// A failed (EPERM) call must not change any observable state.
        /// `setgroups` is a no-op-on-success in our stub, so the only
        /// observable is errno — which the test above confirms.  Here
        /// we additionally verify that *repeated* failed calls all
        /// return the same EPERM and don't drift.
        #[test]
        fn test_setgroups_phase187_repeated_failed_calls_stable() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            for _ in 0..8 {
                errno::set_errno(0);
                assert_eq!(setgroups(2, [10, 20].as_ptr()), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
        }

        // -- Sentinel --------------------------------------------------------

        /// With CAP_SETGID held, the existing EINVAL path still fires
        /// for oversized size.  Confirms the gate is *gated* on the
        /// cap, not an unconditional EPERM addition.
        #[test]
        fn test_setgroups_phase187_with_cap_einval_still_fires() {
            let _g = CapGuard::snapshot();
            // Cap is held by default — don't drop it.
            errno::set_errno(0);
            let groups: [GidT; 1] = [0];
            assert_eq!(setgroups(65537, groups.as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL,
                "With cap held, EINVAL must still surface for bad size");
        }

        /// With CAP_SETGID held, the existing EFAULT path still fires
        /// for size>0 with NULL list.
        #[test]
        fn test_setgroups_phase187_with_cap_efault_still_fires() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setgroups(3, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT,
                "With cap held, EFAULT must still surface for null list");
        }

        /// With CAP_SETGID held, valid call still succeeds.
        #[test]
        fn test_setgroups_phase187_with_cap_valid_call_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            let groups: [GidT; 4] = [100, 200, 300, 400];
            assert_eq!(setgroups(4, groups.as_ptr()), 0);
        }

        // -- Cross-check -----------------------------------------------------

        /// Dropping CAP_SETUID *alone* must NOT cause setgroups to
        /// fail — Linux gates setgroups on CAP_SETGID specifically.
        /// This test pins down the cross-cap invariant so a future
        /// refactor that accidentally probes the wrong cap is caught.
        #[test]
        fn test_setgroups_phase187_setuid_drop_does_not_affect_setgroups() {
            use crate::sys_capability::CAP_SETUID;
            let _g = CapGuard::snapshot();
            // Drop only CAP_SETUID (bit 7), leave CAP_SETGID (bit 6).
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << CAP_SETUID);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            // setgroups still works.
            errno::set_errno(0);
            let groups: [GidT; 2] = [50, 60];
            assert_eq!(setgroups(2, groups.as_ptr()), 0);
        }

        /// Phase 187 errno is EPERM (the `capable()` convention),
        /// matching Linux's `may_setgroups` → `-EPERM`.  Distinct from
        /// the EACCES errno used by Phase 186 (`seccomp` filter
        /// install) — a cross-phase invariant pinning down which gate
        /// uses which errno convention.
        #[test]
        fn test_setgroups_phase187_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgroups(1, [99].as_ptr()), -1);
            let e = errno::get_errno();
            assert_eq!(e, errno::EPERM);
            assert_ne!(e, errno::EACCES,
                "may_setgroups uses EPERM (capable convention), \
                 distinct from seccomp's EACCES");
        }
    }

    // ==================================================================
    // Phase 192: setuid / seteuid gate on CAP_SETUID
    // ==================================================================
    //
    // Linux's `kernel/sys.c::sys_setuid` allows the call when the target
    // uid matches the real, effective, or saved uid OR the caller holds
    // CAP_SETUID; otherwise EPERM.  Our flat single-uid (always 0) model
    // collapses that to "target == 0 always OK; target != 0 needs
    // CAP_SETUID".  Pre-Phase-192 we returned 0 unconditionally, which
    // silently masked sandbox-drop bugs in callers.
    mod setuid_cap_phase192 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 187 (`setgroups_cap_phase187`).
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_setuid() {
            use crate::sys_capability::CAP_SETUID;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SETUID < 32 {
                (lo & !(1u32 << CAP_SETUID), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SETUID - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SETUID");
            assert!(!crate::sys_capability::has_capability(CAP_SETUID));
        }

        // -- Per-error-class --------------------------------------------------

        /// `setuid(1000)` with no CAP_SETUID — used to silently succeed
        /// and made callers think they had dropped to an unprivileged
        /// uid.  Phase 192 turns this into the EPERM Linux reports.
        #[test]
        fn test_setuid_phase192_no_cap_normal_uid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(setuid(1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Same gate at u32::MAX (often the "nobody" sentinel for
        /// invalid mappings in user namespaces).
        #[test]
        fn test_setuid_phase192_no_cap_max_uid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(setuid(u32::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Same gate at uid 1 (the historical "daemon" uid — close to
        /// 0 but not zero, must still trip the cap check).
        #[test]
        fn test_setuid_phase192_no_cap_uid_one_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(setuid(1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `seteuid(1000)` with no CAP_SETUID — same gate, same errno
        /// as `setuid`.  Linux uses the same permission table for both.
        #[test]
        fn test_seteuid_phase192_no_cap_normal_uid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(seteuid(1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `seteuid(u32::MAX)` with no cap — boundary sentinel.
        #[test]
        fn test_seteuid_phase192_no_cap_max_uid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(seteuid(u32::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering / sentinel ---------------------------------------------

        /// `setuid(0)` with no cap — target matches current uid, so the
        /// gate is bypassed via the "match-current-uid" arm of Linux's
        /// table.  Must succeed even without CAP_SETUID.
        #[test]
        fn test_setuid_phase192_no_cap_target_zero_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(setuid(0), 0);
            // Errno is left untouched (still 0 from our reset).
            assert_eq!(errno::get_errno(), 0);
        }

        /// `seteuid(0)` with no cap — same bypass.
        #[test]
        fn test_seteuid_phase192_no_cap_target_zero_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(seteuid(0), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        /// `setuid(0)` with cap held — trivial success.  Pins down that
        /// the cap presence doesn't accidentally invert the result.
        #[test]
        fn test_setuid_phase192_with_cap_target_zero_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setuid(0), 0);
        }

        /// `setuid(1000)` with cap held — the cap-allowed path.
        #[test]
        fn test_setuid_phase192_with_cap_normal_uid_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setuid(1000), 0);
        }

        /// `seteuid(1000)` with cap held — the cap-allowed path.
        #[test]
        fn test_seteuid_phase192_with_cap_normal_uid_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(seteuid(1000), 0);
        }

        // -- Workflow --------------------------------------------------------

        /// Container-style sandbox drop: cap held → drop privilege call
        /// would succeed; drop cap → subsequent attempt EPERMs.  This
        /// is the workflow whose bug pre-Phase-192 was silent.
        #[test]
        fn test_setuid_phase192_sandbox_drop_workflow() {
            let _g = CapGuard::snapshot();
            // 1. Privileged code can change uid.
            errno::set_errno(0);
            assert_eq!(setuid(1000), 0);
            // 2. Sandbox drops CAP_SETUID.
            drop_cap_setuid();
            // 3. A confused caller in the sandbox tries to "re-drop" to
            //    another uid — now correctly reports EPERM instead of
            //    silently succeeding.
            errno::set_errno(0);
            assert_eq!(setuid(2000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // 4. But returning to uid 0 (the "still-current" sentinel)
            //    remains allowed via the match-current arm.
            errno::set_errno(0);
            assert_eq!(setuid(0), 0);
        }

        // -- Buggy-caller ----------------------------------------------------

        /// Caller forgot to clear errno — fresh EPERM still surfaces.
        #[test]
        fn test_setuid_phase192_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(errno::ENOENT);
            assert_eq!(setuid(42), -1);
            assert_eq!(errno::get_errno(), errno::EPERM,
                "Stale ENOENT must be overwritten with EPERM");
        }

        /// Caller passes the "max minus one" sentinel — still EPERM.
        #[test]
        fn test_setuid_phase192_buggy_caller_max_minus_one_still_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(setuid(u32::MAX - 1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery --------------------------------------------------------

        /// EPERM → restore cap via guard drop → next call succeeds.
        #[test]
        fn test_setuid_phase192_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_setuid();
                errno::set_errno(0);
                assert_eq!(setuid(1234), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            } // cap restored on guard drop.
            errno::set_errno(0);
            assert_eq!(setuid(1234), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        // -- No-side-effect --------------------------------------------------

        /// Repeated failed calls are stable — same -1 / EPERM, no drift.
        #[test]
        fn test_setuid_phase192_repeated_failed_calls_stable() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            for _ in 0..8 {
                errno::set_errno(0);
                assert_eq!(setuid(7777), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
        }

        /// A failed setuid must not perturb `getuid`/`geteuid` — they
        /// remain 0 (the single-user model is unchanged).
        #[test]
        fn test_setuid_phase192_failed_call_no_observable_uid_change() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            let pre_uid = getuid();
            let pre_euid = geteuid();
            errno::set_errno(0);
            assert_eq!(setuid(1234), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            assert_eq!(getuid(), pre_uid);
            assert_eq!(geteuid(), pre_euid);
        }

        // -- Cross-check -----------------------------------------------------

        /// Dropping CAP_SETGID alone must NOT affect setuid — Linux
        /// gates setuid on CAP_SETUID specifically.  Pins the
        /// cross-cap invariant so a future probe of the wrong cap is
        /// caught.
        #[test]
        fn test_setuid_phase192_setgid_drop_does_not_affect_setuid() {
            use crate::sys_capability::CAP_SETGID;
            let _g = CapGuard::snapshot();
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << CAP_SETGID);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            // setuid still works.
            errno::set_errno(0);
            assert_eq!(setuid(1500), 0);
            assert_eq!(seteuid(1500), 0);
        }

        /// Dropping CAP_SETUID alone must NOT affect setgid — the
        /// inverse cross-cap invariant.  setgid is still a permissive
        /// stub (Phase 193 will gate it), but the test here confirms
        /// the Phase 192 gate doesn't accidentally also block setgid.
        #[test]
        fn test_setuid_phase192_drop_does_not_affect_setgid() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            // setgid is still a permissive stub — Phase 193 territory.
            // Confirm the Phase 192 cap drop didn't accidentally also
            // gate setgid.
            errno::set_errno(0);
            assert_eq!(setgid(1000), 0);
            assert_eq!(setegid(1000), 0);
        }

        /// Phase 192 errno is EPERM (the `capable()` convention),
        /// matching Linux's `sys_setuid` → `-EPERM` for the cap path.
        /// Distinct from EACCES.
        #[test]
        fn test_setuid_phase192_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            errno::set_errno(0);
            assert_eq!(setuid(99), -1);
            let e = errno::get_errno();
            assert_eq!(e, errno::EPERM);
            assert_ne!(e, errno::EACCES,
                "sys_setuid uses EPERM (capable convention), \
                 distinct from LSM-style EACCES");
        }

        /// `getuid`/`geteuid` are not gated by CAP_SETUID — they
        /// always succeed and return 0 regardless of cap state.
        #[test]
        fn test_setuid_phase192_getters_unaffected_by_cap_drop() {
            let _g = CapGuard::snapshot();
            drop_cap_setuid();
            assert_eq!(getuid(), 0);
            assert_eq!(geteuid(), 0);
        }
    }

    // ==================================================================
    // Phase 193: setgid / setegid gate on CAP_SETGID
    // ==================================================================
    //
    // Companion to Phase 192's setuid/seteuid gate.  Linux's
    // `kernel/sys.c::sys_setgid` allows the call when the target gid
    // matches the real, effective, or saved gid OR the caller holds
    // CAP_SETGID; otherwise EPERM.  Our flat single-gid (always 0)
    // model collapses that to "target == 0 always OK; target != 0
    // needs CAP_SETGID".  Pre-Phase-193 we returned 0 unconditionally,
    // silently masking group-drop bugs in callers.
    mod setgid_cap_phase193 {
        use super::*;

        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_setgid() {
            use crate::sys_capability::CAP_SETGID;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SETGID < 32 {
                (lo & !(1u32 << CAP_SETGID), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SETGID - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SETGID");
            assert!(!crate::sys_capability::has_capability(CAP_SETGID));
        }

        // -- Per-error-class --------------------------------------------------

        #[test]
        fn test_setgid_phase193_no_cap_normal_gid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setgid_phase193_no_cap_max_gid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(u32::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setgid_phase193_no_cap_gid_one_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setegid_phase193_no_cap_normal_gid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setegid(1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setegid_phase193_no_cap_max_gid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setegid(u32::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering / sentinel ---------------------------------------------

        #[test]
        fn test_setgid_phase193_no_cap_target_zero_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(0), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        #[test]
        fn test_setegid_phase193_no_cap_target_zero_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setegid(0), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        #[test]
        fn test_setgid_phase193_with_cap_target_zero_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setgid(0), 0);
        }

        #[test]
        fn test_setgid_phase193_with_cap_normal_gid_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setgid(1000), 0);
        }

        #[test]
        fn test_setegid_phase193_with_cap_normal_gid_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setegid(1000), 0);
        }

        // -- Workflow --------------------------------------------------------

        /// Container-style group-drop: cap held → change succeeds;
        /// drop cap → subsequent attempt EPERMs.
        #[test]
        fn test_setgid_phase193_sandbox_drop_workflow() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setgid(1000), 0);
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(2000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            errno::set_errno(0);
            assert_eq!(setgid(0), 0);
        }

        // -- Buggy-caller ----------------------------------------------------

        #[test]
        fn test_setgid_phase193_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(errno::ENOENT);
            assert_eq!(setgid(42), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setgid_phase193_buggy_caller_max_minus_one_still_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(u32::MAX - 1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery --------------------------------------------------------

        #[test]
        fn test_setgid_phase193_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_setgid();
                errno::set_errno(0);
                assert_eq!(setgid(1234), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
            errno::set_errno(0);
            assert_eq!(setgid(1234), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        // -- No-side-effect --------------------------------------------------

        #[test]
        fn test_setgid_phase193_repeated_failed_calls_stable() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            for _ in 0..8 {
                errno::set_errno(0);
                assert_eq!(setgid(7777), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
        }

        #[test]
        fn test_setgid_phase193_failed_call_no_observable_gid_change() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            let pre_gid = getgid();
            let pre_egid = getegid();
            errno::set_errno(0);
            assert_eq!(setgid(1234), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            assert_eq!(getgid(), pre_gid);
            assert_eq!(getegid(), pre_egid);
        }

        // -- Cross-check -----------------------------------------------------

        /// Dropping CAP_SETUID alone must NOT affect setgid — Linux
        /// gates setgid on CAP_SETGID specifically.
        #[test]
        fn test_setgid_phase193_setuid_drop_does_not_affect_setgid() {
            use crate::sys_capability::CAP_SETUID;
            let _g = CapGuard::snapshot();
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << CAP_SETUID);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(setgid(1500), 0);
            assert_eq!(setegid(1500), 0);
        }

        /// Dropping CAP_SETGID alone must NOT affect setuid — inverse
        /// cross-cap invariant.  setuid is fully gated by Phase 192,
        /// but only on its own CAP_SETUID.
        #[test]
        fn test_setgid_phase193_drop_does_not_affect_setuid() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            // CAP_SETUID is still held — setuid still works.
            errno::set_errno(0);
            assert_eq!(setuid(1000), 0);
            assert_eq!(seteuid(1000), 0);
        }

        #[test]
        fn test_setgid_phase193_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(99), -1);
            let e = errno::get_errno();
            assert_eq!(e, errno::EPERM);
            assert_ne!(e, errno::EACCES,
                "sys_setgid uses EPERM (capable convention), \
                 distinct from LSM-style EACCES");
        }

        /// getgid/getegid unaffected by CAP_SETGID drop.
        #[test]
        fn test_setgid_phase193_getters_unaffected_by_cap_drop() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            assert_eq!(getgid(), 0);
            assert_eq!(getegid(), 0);
        }

        /// Pin the cross-phase errno invariant: Phase 187
        /// (`setgroups`) also uses CAP_SETGID + EPERM.  A test here
        /// confirms dropping CAP_SETGID gates BOTH setgid AND
        /// setgroups simultaneously — the cap is shared between
        /// these gid-touching syscalls in Linux's source.
        #[test]
        fn test_setgid_phase193_drop_also_gates_setgroups() {
            let _g = CapGuard::snapshot();
            drop_cap_setgid();
            errno::set_errno(0);
            assert_eq!(setgid(1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            errno::set_errno(0);
            assert_eq!(setgroups(0, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }
    }

    // ==================================================================
    // Phase 194: setreuid / setregid gate on CAP_SETUID / CAP_SETGID
    // ==================================================================
    //
    // Linux's `sys_setreuid` / `sys_setregid` permission-check each
    // field independently.  The `(uid_t)-1` / `(gid_t)-1` sentinel
    // ("leave alone") bypasses its field's check.  Each non-sentinel
    // field must either match a currently-held id (real / effective /
    // saved) OR the caller must hold the relevant SET-id cap.  In
    // our flat single-id (always 0) model: value == 0 or value ==
    // MAX always OK; any other value requires the cap.
    //
    // The order of evaluation matters for ordering tests: Linux
    // checks ruid before euid, so a (bad_ruid, bad_euid) call EPERMs
    // on the ruid check and the euid value is irrelevant.
    mod setreuid_cap_phase194 {
        use super::*;

        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap(cap: u32) {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed when dropping cap {cap}");
            assert!(!crate::sys_capability::has_capability(cap));
        }

        // -- Per-error-class: setreuid -------------------------------------

        #[test]
        fn test_setreuid_phase194_no_cap_bad_ruid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(1000, UidT::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setreuid_phase194_no_cap_bad_euid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(UidT::MAX, 1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setreuid_phase194_no_cap_both_bad_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(1000, 2000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Per-error-class: setregid -------------------------------------

        #[test]
        fn test_setregid_phase194_no_cap_bad_rgid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setregid(1000, GidT::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setregid_phase194_no_cap_bad_egid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setregid(GidT::MAX, 1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setregid_phase194_no_cap_both_bad_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setregid(1000, 2000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering: ruid checked before euid ----------------------------

        /// With cap dropped, (bad_ruid, bad_euid) EPERMs on the ruid
        /// arm — the euid value is irrelevant.  Matches Linux's
        /// top-down evaluation order in sys_setreuid.
        #[test]
        fn test_setreuid_phase194_ruid_check_before_euid_check() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            // ruid is bad → EPERM regardless of what euid is.
            errno::set_errno(0);
            assert_eq!(setreuid(1000, 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Mirror with a zero ruid but bad euid — must reach the
            // euid check (also EPERM, but via the second arm).
            errno::set_errno(0);
            assert_eq!(setreuid(0, 1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Sentinel (-1) bypass -----------------------------------------

        /// `(MAX, MAX)` = both fields skip = always succeeds, even
        /// without any cap.  Classic "noop probe" pattern.
        #[test]
        fn test_setreuid_phase194_both_sentinels_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(UidT::MAX, UidT::MAX), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        #[test]
        fn test_setregid_phase194_both_sentinels_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setregid(GidT::MAX, GidT::MAX), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        /// `(MAX, 0)` and `(0, MAX)` — one sentinel + the "matches
        /// current" zero.  No cap needed.
        #[test]
        fn test_setreuid_phase194_sentinel_plus_zero_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(UidT::MAX, 0), 0);
            assert_eq!(setreuid(0, UidT::MAX), 0);
        }

        /// `(0, 0)` — both match current uid, no cap needed.
        #[test]
        fn test_setreuid_phase194_both_zero_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(0, 0), 0);
        }

        // -- With-cap success path ----------------------------------------

        #[test]
        fn test_setreuid_phase194_with_cap_arbitrary_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setreuid(1000, 2000), 0);
            assert_eq!(setreuid(u32::MAX - 1, u32::MAX - 2), 0);
        }

        #[test]
        fn test_setregid_phase194_with_cap_arbitrary_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setregid(1000, 2000), 0);
        }

        // -- Workflow ------------------------------------------------------

        /// Container sandbox: privileged setreuid runs fine; after
        /// dropping CAP_SETUID, only the sentinel/zero paths are
        /// still open.  Mirrors libc's privilege-drop sequence
        /// (setreuid(uid, uid) then later setreuid(-1, -1) probes).
        #[test]
        fn test_setreuid_phase194_sandbox_drop_workflow() {
            let _g = CapGuard::snapshot();
            // 1. Privileged: drop to "1000/1000".
            errno::set_errno(0);
            assert_eq!(setreuid(1000, 1000), 0);
            // 2. Sandbox drops CAP_SETUID.
            drop_cap(crate::sys_capability::CAP_SETUID);
            // 3. Probe with sentinels — still works.
            errno::set_errno(0);
            assert_eq!(setreuid(UidT::MAX, UidT::MAX), 0);
            // 4. Try to re-escalate to a different uid — EPERM.
            errno::set_errno(0);
            assert_eq!(setreuid(2000, UidT::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy-caller --------------------------------------------------

        /// Caller forgot to clear errno — fresh EPERM still
        /// surfaces, stale errno wiped.
        #[test]
        fn test_setreuid_phase194_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(errno::ENOENT);
            assert_eq!(setreuid(7, 8), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Both fields one-shy-of-MAX (NOT the sentinel) — still
        /// EPERM.  Verifies the gate doesn't accidentally treat
        /// MAX-1 as a sentinel.
        #[test]
        fn test_setreuid_phase194_max_minus_one_not_sentinel() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(u32::MAX - 1, u32::MAX - 1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery ------------------------------------------------------

        #[test]
        fn test_setreuid_phase194_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap(crate::sys_capability::CAP_SETUID);
                errno::set_errno(0);
                assert_eq!(setreuid(1234, 5678), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
            errno::set_errno(0);
            assert_eq!(setreuid(1234, 5678), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        // -- No-side-effect ------------------------------------------------

        #[test]
        fn test_setreuid_phase194_repeated_failed_calls_stable() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            for _ in 0..8 {
                errno::set_errno(0);
                assert_eq!(setreuid(3, 4), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
        }

        #[test]
        fn test_setreuid_phase194_failed_call_no_observable_uid_change() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            let pre_uid = getuid();
            let pre_euid = geteuid();
            errno::set_errno(0);
            assert_eq!(setreuid(9, 10), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            assert_eq!(getuid(), pre_uid);
            assert_eq!(geteuid(), pre_euid);
        }

        // -- Cross-check ---------------------------------------------------

        /// Dropping CAP_SETGID alone must NOT affect setreuid — uid
        /// vs gid cap separation.
        #[test]
        fn test_setreuid_phase194_setgid_drop_does_not_affect_setreuid() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setreuid(1000, 2000), 0);
        }

        /// Dropping CAP_SETUID alone must NOT affect setregid.
        #[test]
        fn test_setregid_phase194_setuid_drop_does_not_affect_setregid() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setregid(1000, 2000), 0);
        }

        /// Dropping CAP_SETUID gates BOTH setuid AND setreuid in one
        /// shot — they share the cap in Linux's source.
        #[test]
        fn test_setreuid_phase194_drop_also_gates_setuid() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setuid(1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            errno::set_errno(0);
            assert_eq!(setreuid(1000, 1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            errno::set_errno(0);
            assert_eq!(seteuid(1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Dropping CAP_SETGID gates setgid, setegid, setregid, AND
        /// setgroups — the full gid-mutating fan-out.
        #[test]
        fn test_setregid_phase194_drop_also_gates_all_gid_setters() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            for (rc, name) in [
                (setgid(1000), "setgid"),
                (setegid(1000), "setegid"),
                (setregid(1000, 2000), "setregid"),
                (setgroups(0, core::ptr::null()), "setgroups"),
            ] {
                assert_eq!(rc, -1, "{name} must EPERM without CAP_SETGID");
            }
        }

        #[test]
        fn test_setreuid_phase194_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setreuid(99, 88), -1);
            let e = errno::get_errno();
            assert_eq!(e, errno::EPERM);
            assert_ne!(e, errno::EACCES);
        }
    }

    // ==================================================================
    // Phase 195: setresuid / setresgid gate on CAP_SETUID / CAP_SETGID
    // ==================================================================
    //
    // The three-arg saved-id variants.  Linux's sys_setresuid uses a
    // single CAP_SETUID outer-guard: if the cap is held, all three
    // fields are accepted; otherwise each non-sentinel field must
    // match a currently-held id.  Order: ruid → euid → suid.  Our
    // flat single-uid (always 0) model collapses to "value == 0 or
    // value == MAX always OK; any other value requires the cap".
    //
    // This was the highest-stakes silent-success bug in the
    // setuid-family series — setresuid is what sandbox/jail code
    // uses to clamp all three uids simultaneously and is the
    // recommended privilege-drop syscall in modern Linux APIs.
    mod setresuid_cap_phase195 {
        use super::*;

        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap(cap: u32) {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed when dropping cap {cap}");
            assert!(!crate::sys_capability::has_capability(cap));
        }

        // -- Per-error-class: each field independently fails ---------------

        #[test]
        fn test_setresuid_phase195_no_cap_bad_ruid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(1000, UidT::MAX, UidT::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setresuid_phase195_no_cap_bad_euid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(UidT::MAX, 1000, UidT::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setresuid_phase195_no_cap_bad_suid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(UidT::MAX, UidT::MAX, 1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setresgid_phase195_no_cap_bad_rgid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setresgid(1000, GidT::MAX, GidT::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setresgid_phase195_no_cap_bad_egid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setresgid(GidT::MAX, 1000, GidT::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_setresgid_phase195_no_cap_bad_sgid_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setresgid(GidT::MAX, GidT::MAX, 1000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering: ruid → euid → suid ----------------------------------

        /// (bad, bad, bad) → EPERM on ruid arm; later fields never
        /// reached.  Matches Linux's top-down evaluation.
        #[test]
        fn test_setresuid_phase195_all_bad_eperms_on_first_arm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(1, 2, 3), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// (0, bad, bad) skips the ruid arm and EPERMs on euid arm.
        #[test]
        fn test_setresuid_phase195_zero_ruid_reaches_euid_arm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(0, 2, 3), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// (0, 0, bad) reaches the suid arm and EPERMs there.
        #[test]
        fn test_setresuid_phase195_zero_ruid_euid_reaches_suid_arm() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(0, 0, 3), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Sentinel (-1) bypass -----------------------------------------

        /// All three sentinels — always succeeds with no cap.
        #[test]
        fn test_setresuid_phase195_all_sentinels_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(UidT::MAX, UidT::MAX, UidT::MAX), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        #[test]
        fn test_setresgid_phase195_all_sentinels_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setresgid(GidT::MAX, GidT::MAX, GidT::MAX), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        /// All zeros — every field matches current, no cap needed.
        #[test]
        fn test_setresuid_phase195_all_zero_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(0, 0, 0), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        /// Mix of sentinel + zero — every combination passes.
        #[test]
        fn test_setresuid_phase195_sentinel_zero_mix_succeed_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(0, UidT::MAX, 0), 0);
            assert_eq!(setresuid(UidT::MAX, 0, UidT::MAX), 0);
            assert_eq!(setresuid(UidT::MAX, UidT::MAX, 0), 0);
            assert_eq!(setresuid(0, 0, UidT::MAX), 0);
        }

        // -- With-cap success path ----------------------------------------

        #[test]
        fn test_setresuid_phase195_with_cap_arbitrary_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setresuid(1, 2, 3), 0);
            assert_eq!(setresuid(u32::MAX - 1, u32::MAX - 2, u32::MAX - 3), 0);
        }

        #[test]
        fn test_setresgid_phase195_with_cap_arbitrary_succeeds() {
            let _g = CapGuard::snapshot();
            errno::set_errno(0);
            assert_eq!(setresgid(1, 2, 3), 0);
        }

        // -- Workflow ------------------------------------------------------

        /// The canonical sandbox-drop pattern using setresuid:
        /// `setresuid(N, N, N)` to clamp all three.  Pre-Phase-195
        /// this silently succeeded even without CAP_SETUID — exactly
        /// the privilege-drop boundary callers expect to be enforced.
        #[test]
        fn test_setresuid_phase195_sandbox_drop_workflow() {
            let _g = CapGuard::snapshot();
            // 1. Privileged: clamp to uid 1000 across all three.
            errno::set_errno(0);
            assert_eq!(setresuid(1000, 1000, 1000), 0);
            // 2. Sandbox drops CAP_SETUID.
            drop_cap(crate::sys_capability::CAP_SETUID);
            // 3. Sentinel probe — still works.
            errno::set_errno(0);
            assert_eq!(setresuid(UidT::MAX, UidT::MAX, UidT::MAX), 0);
            // 4. Confused re-clamp to uid 2000 — EPERMs.
            errno::set_errno(0);
            assert_eq!(setresuid(2000, 2000, 2000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy-caller --------------------------------------------------

        #[test]
        fn test_setresuid_phase195_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(errno::ENOENT);
            assert_eq!(setresuid(7, 8, 9), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// (MAX-1, MAX-1, MAX-1) NOT treated as the sentinel triple.
        #[test]
        fn test_setresuid_phase195_max_minus_one_not_sentinel() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(
                setresuid(u32::MAX - 1, u32::MAX - 1, u32::MAX - 1),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery ------------------------------------------------------

        #[test]
        fn test_setresuid_phase195_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap(crate::sys_capability::CAP_SETUID);
                errno::set_errno(0);
                assert_eq!(setresuid(1, 2, 3), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
            errno::set_errno(0);
            assert_eq!(setresuid(1, 2, 3), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        // -- No-side-effect ------------------------------------------------

        #[test]
        fn test_setresuid_phase195_repeated_failed_calls_stable() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            for _ in 0..8 {
                errno::set_errno(0);
                assert_eq!(setresuid(3, 4, 5), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
        }

        #[test]
        fn test_setresuid_phase195_failed_call_no_observable_uid_change() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            let pre_uid = getuid();
            let pre_euid = geteuid();
            errno::set_errno(0);
            assert_eq!(setresuid(9, 10, 11), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            assert_eq!(getuid(), pre_uid);
            assert_eq!(geteuid(), pre_euid);
        }

        // -- Cross-check ---------------------------------------------------

        /// Dropping CAP_SETGID alone must NOT affect setresuid.
        #[test]
        fn test_setresuid_phase195_setgid_drop_does_not_affect_setresuid() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            errno::set_errno(0);
            assert_eq!(setresuid(1000, 2000, 3000), 0);
        }

        /// Dropping CAP_SETUID alone must NOT affect setresgid.
        #[test]
        fn test_setresgid_phase195_setuid_drop_does_not_affect_setresgid() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresgid(1000, 2000, 3000), 0);
        }

        /// Dropping CAP_SETUID gates the full uid-setter fan-out:
        /// setuid, seteuid, setreuid, setresuid all EPERM.
        #[test]
        fn test_setresuid_phase195_drop_gates_all_uid_setters() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            for (rc, name) in [
                (setuid(1000), "setuid"),
                (seteuid(1000), "seteuid"),
                (setreuid(1000, 2000), "setreuid"),
                (setresuid(1000, 2000, 3000), "setresuid"),
            ] {
                assert_eq!(rc, -1, "{name} must EPERM without CAP_SETUID");
            }
        }

        /// Dropping CAP_SETGID gates the full gid-setter fan-out:
        /// setgid, setegid, setregid, setresgid, setgroups all EPERM.
        #[test]
        fn test_setresgid_phase195_drop_gates_all_gid_setters() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            for (rc, name) in [
                (setgid(1000), "setgid"),
                (setegid(1000), "setegid"),
                (setregid(1000, 2000), "setregid"),
                (setresgid(1000, 2000, 3000), "setresgid"),
                (setgroups(0, core::ptr::null()), "setgroups"),
            ] {
                assert_eq!(rc, -1, "{name} must EPERM without CAP_SETGID");
            }
        }

        #[test]
        fn test_setresuid_phase195_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            errno::set_errno(0);
            assert_eq!(setresuid(99, 88, 77), -1);
            let e = errno::get_errno();
            assert_eq!(e, errno::EPERM);
            assert_ne!(e, errno::EACCES);
        }

        /// getresuid/getresgid are pure readers — unaffected by cap
        /// drops.  Pin the read-vs-write cap invariant.
        #[test]
        fn test_setresuid_phase195_getters_unaffected_by_cap_drop() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            drop_cap(crate::sys_capability::CAP_SETGID);
            let mut r: UidT = 99;
            let mut e: UidT = 99;
            let mut s: UidT = 99;
            assert_eq!(getresuid(&raw mut r, &raw mut e, &raw mut s), 0);
            assert_eq!((r, e, s), (0, 0, 0));
            let mut rg: GidT = 99;
            let mut eg: GidT = 99;
            let mut sg: GidT = 99;
            assert_eq!(
                getresgid(&raw mut rg, &raw mut eg, &raw mut sg),
                0
            );
            assert_eq!((rg, eg, sg), (0, 0, 0));
        }
    }

    // ------------------------------------------------------------------
    // ualarm — stub returns 0
    // ------------------------------------------------------------------

    #[test]
    fn test_ualarm_returns_zero() {
        assert_eq!(ualarm(100_000, 0), 0);
    }

    #[test]
    fn test_ualarm_with_interval() {
        assert_eq!(ualarm(100_000, 50_000), 0);
    }

    // ------------------------------------------------------------------
    // chroot — stub returns ENOSYS
    // ------------------------------------------------------------------

    #[test]
    fn test_chroot_enosys() {
        errno::set_errno(0);
        assert_eq!(chroot(b"/\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_chroot_null_path() {
        // chroot with null should fail (the stub returns ENOSYS
        // regardless of arguments, so we just verify it returns -1).
        assert_eq!(chroot(core::ptr::null()), -1);
    }

    // ------------------------------------------------------------------
    // sync — void function, just verify no crash
    // ------------------------------------------------------------------

    #[test]
    fn test_sync_no_crash() {
        sync();
        // sync is void — if we got here, it succeeded.
    }

    // ------------------------------------------------------------------
    // getloadavg — fills 0.0 values
    // ------------------------------------------------------------------

    #[test]
    fn test_getloadavg_one() {
        let mut avg = [99.0f64; 1];
        let ret = getloadavg(avg.as_mut_ptr(), 1);
        assert_eq!(ret, 1);
        assert_eq!(avg[0], 0.0);
    }

    #[test]
    fn test_getloadavg_three() {
        let mut avg = [99.0f64; 3];
        let ret = getloadavg(avg.as_mut_ptr(), 3);
        assert_eq!(ret, 3);
        assert_eq!(avg[0], 0.0);
        assert_eq!(avg[1], 0.0);
        assert_eq!(avg[2], 0.0);
    }

    #[test]
    fn test_getloadavg_clamped_to_three() {
        let mut avg = [99.0f64; 5];
        let ret = getloadavg(avg.as_mut_ptr(), 5);
        assert_eq!(ret, 3, "Should clamp to 3 (POSIX max)");
        // Only first 3 should be filled.
        assert_eq!(avg[0], 0.0);
        assert_eq!(avg[1], 0.0);
        assert_eq!(avg[2], 0.0);
        assert_eq!(avg[3], 99.0, "Element 3 should be untouched");
    }

    #[test]
    fn test_getloadavg_null() {
        assert_eq!(getloadavg(core::ptr::null_mut(), 1), -1);
    }

    #[test]
    fn test_getloadavg_zero_nelem() {
        let mut avg = [0.0f64; 1];
        assert_eq!(getloadavg(avg.as_mut_ptr(), 0), -1);
    }

    #[test]
    fn test_getloadavg_negative_nelem() {
        let mut avg = [0.0f64; 1];
        assert_eq!(getloadavg(avg.as_mut_ptr(), -1), -1);
    }

    // ------------------------------------------------------------------
    // getrandom
    // ------------------------------------------------------------------

    #[test]
    fn test_getrandom_null() {
        errno::set_errno(0);
        assert_eq!(getrandom(core::ptr::null_mut(), 10, 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getrandom_zero_len() {
        let mut buf = [0u8; 1];
        let ret = getrandom(buf.as_mut_ptr(), 0, 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_getrandom_fills_buffer() {
        let mut buf = [0u8; 32];
        let ret = getrandom(buf.as_mut_ptr(), 32, 0);
        assert_eq!(ret, 32);
        // It's theoretically possible all bytes are 0, but extremely
        // unlikely for 32 bytes of random data.
        // Just verify the call succeeded and returned the right count.
    }

    #[test]
    fn test_getrandom_overflow_len() {
        let mut buf = [0u8; 1];
        errno::set_errno(0);
        let ret = getrandom(buf.as_mut_ptr(), usize::MAX, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrandom_with_flags() {
        let mut buf = [0u8; 8];
        let ret = getrandom(buf.as_mut_ptr(), 8, GRND_NONBLOCK);
        assert_eq!(ret, 8);
        let ret2 = getrandom(buf.as_mut_ptr(), 8, GRND_RANDOM);
        assert_eq!(ret2, 8);
    }

    // ------------------------------------------------------------------
    // getentropy
    // ------------------------------------------------------------------

    #[test]
    fn test_getentropy_null() {
        errno::set_errno(0);
        assert_eq!(getentropy(core::ptr::null_mut(), 10), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getentropy_too_large() {
        let mut buf = [0u8; 257];
        errno::set_errno(0);
        assert_eq!(getentropy(buf.as_mut_ptr(), 257), -1);
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    #[test]
    fn test_getentropy_max_256() {
        let mut buf = [0u8; 256];
        assert_eq!(getentropy(buf.as_mut_ptr(), 256), 0);
    }

    #[test]
    fn test_getentropy_zero() {
        let mut buf = [0u8; 1];
        assert_eq!(getentropy(buf.as_mut_ptr(), 0), 0);
    }

    #[test]
    fn test_getentropy_small() {
        let mut buf = [0u8; 16];
        assert_eq!(getentropy(buf.as_mut_ptr(), 16), 0);
    }

    // ------------------------------------------------------------------
    // GRND flag constants
    // ------------------------------------------------------------------

    #[test]
    fn test_grnd_constants() {
        assert_eq!(GRND_NONBLOCK, 1);
        assert_eq!(GRND_RANDOM, 2);
        assert_ne!(GRND_NONBLOCK, GRND_RANDOM);
    }

    // ------------------------------------------------------------------
    // getcwd
    // ------------------------------------------------------------------

    #[test]
    fn test_getcwd_null_buf() {
        errno::set_errno(0);
        let ret = getcwd(core::ptr::null_mut(), 100);
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getcwd_zero_size() {
        let mut buf = [0u8; 100];
        errno::set_errno(0);
        let ret = getcwd(buf.as_mut_ptr(), 0);
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getcwd_buffer_too_small() {
        let mut buf = [0u8; 1];
        // CWD is at least "/" (1 byte + null = 2), so size=1 should fail.
        errno::set_errno(0);
        let ret = getcwd(buf.as_mut_ptr(), 1);
        assert!(ret.is_null());
        assert_eq!(errno::get_errno(), errno::ERANGE);
    }

    #[test]
    fn test_getcwd_succeeds() {
        let mut buf = [0u8; PATH_MAX];
        let ret = getcwd(buf.as_mut_ptr(), PATH_MAX);
        assert!(!ret.is_null(), "getcwd should succeed with PATH_MAX buffer");
        assert_eq!(ret, buf.as_mut_ptr());
        // Result should be null-terminated.
        let nul_pos = buf.iter().position(|&b| b == 0);
        assert!(nul_pos.is_some(), "getcwd result should be null-terminated");
        // Should start with '/'.
        assert_eq!(buf[0], b'/', "CWD should start with '/'");
    }

    // ------------------------------------------------------------------
    // chdir — error paths (actual dir changes need kernel)
    // ------------------------------------------------------------------

    #[test]
    fn test_chdir_null_path() {
        errno::set_errno(0);
        assert_eq!(chdir(core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_chdir_empty_path() {
        errno::set_errno(0);
        assert_eq!(chdir(b"\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    // ------------------------------------------------------------------
    // fchdir — error paths
    // ------------------------------------------------------------------

    #[test]
    fn test_fchdir_invalid_fd() {
        errno::set_errno(0);
        assert_eq!(fchdir(-1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ------------------------------------------------------------------
    // Standard file descriptor constants
    // ------------------------------------------------------------------

    #[test]
    fn test_stdio_fileno_constants() {
        assert_eq!(STDIN_FILENO, 0);
        assert_eq!(STDOUT_FILENO, 1);
        assert_eq!(STDERR_FILENO, 2);
    }

    // ------------------------------------------------------------------
    // getgroups with non-zero size
    // ------------------------------------------------------------------

    #[test]
    fn test_getgroups_with_buffer() {
        let mut groups: [GidT; 5] = [99; 5];
        let ret = getgroups(5, groups.as_mut_ptr());
        assert_eq!(ret, 0, "getgroups should return 0 (no supplementary groups)");
    }

    // -- Phase 94: getgroups size validation --

    #[test]
    fn test_getgroups_phase94_negative_size_einval() {
        crate::errno::set_errno(0);
        let ret = getgroups(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getgroups_phase94_int_min_size_einval() {
        crate::errno::set_errno(0);
        let ret = getgroups(i32::MIN, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getgroups_phase94_zero_size_with_null_list_ok() {
        // POSIX query form: size==0 + NULL list → return count (0).
        crate::errno::set_errno(0);
        let ret = getgroups(0, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getgroups_phase94_positive_size_with_null_list_ok() {
        // We have 0 supplementary groups → nothing to copy →
        // list pointer is never dereferenced, matching Linux's
        // copy_to_user-only-when-ngroups>0 behaviour.
        crate::errno::set_errno(0);
        let ret = getgroups(8, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_getgroups_phase94_positive_size_with_valid_list_ok() {
        let mut groups: [GidT; 16] = [0xDEAD; 16];
        crate::errno::set_errno(0);
        let ret = getgroups(16, groups.as_mut_ptr());
        assert_eq!(ret, 0);
        // The buffer must not have been written (we have 0 groups).
        assert_eq!(groups[0], 0xDEAD);
    }

    #[test]
    fn test_getgroups_phase94_einval_then_valid_progression() {
        crate::errno::set_errno(0);
        let bad = getgroups(-5, core::ptr::null_mut());
        assert_eq!(bad, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Subsequent valid call still works.
        crate::errno::set_errno(0);
        let good = getgroups(0, core::ptr::null_mut());
        assert_eq!(good, 0);
    }

    #[test]
    fn test_getgroups_phase94_buggy_caller_signed_overflow() {
        // Caller computed `nbytes / sizeof(gid_t)` with signed math and
        // it wrapped negative.  Linux reports EINVAL — so do we.
        crate::errno::set_errno(0);
        let buggy_size: i32 = -42;
        let ret = getgroups(buggy_size, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ------------------------------------------------------------------
    // sysinfo
    // ------------------------------------------------------------------

    #[test]
    fn test_sysinfo_null() {
        errno::set_errno(0);
        assert_eq!(sysinfo(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_sysinfo_fills_struct() {
        let mut info = core::mem::MaybeUninit::<Sysinfo>::zeroed();
        let ret = sysinfo(info.as_mut_ptr());
        assert_eq!(ret, 0);
        let info = unsafe { info.assume_init() };
        assert!(info.totalram > 0, "totalram should be > 0");
        assert!(info.freeram > 0, "freeram should be > 0");
        assert_eq!(info.mem_unit, 1, "mem_unit should be 1 (byte units)");
        assert!(info.procs >= 1, "procs should be at least 1");
    }

    #[test]
    fn test_sysinfo_uptime_non_negative() {
        let mut info = core::mem::MaybeUninit::<Sysinfo>::zeroed();
        let _ = sysinfo(info.as_mut_ptr());
        let info = unsafe { info.assume_init() };
        assert!(info.uptime >= 0, "uptime should be non-negative");
    }

    #[test]
    fn test_sysinfo_size() {
        // Sysinfo should be reasonably sized.
        let size = core::mem::size_of::<Sysinfo>();
        assert!(size >= 64, "Sysinfo should be at least 64 bytes, got {size}");
    }

    #[test]
    fn test_sysinfo_procs_host_fallback_is_one() {
        // On the host (cargo test target), read_process_count returns 1
        // unconditionally — there is no kernel to query.  This pins the
        // host fallback so it can't silently regress.
        let mut info = core::mem::MaybeUninit::<Sysinfo>::zeroed();
        let _ = sysinfo(info.as_mut_ptr());
        let info = unsafe { info.assume_init() };
        assert_eq!(info.procs, 1, "host fallback should report procs == 1");
    }

    // ------------------------------------------------------------------
    // mntent stubs
    // ------------------------------------------------------------------

    #[test]
    fn test_setmntent_returns_null() {
        let ret = setmntent(b"/etc/mtab\0".as_ptr(), b"r\0".as_ptr());
        assert!(ret.is_null(), "setmntent should return null (no mount table)");
    }

    #[test]
    fn test_getmntent_returns_null() {
        let ret = getmntent(core::ptr::null_mut());
        assert!(ret.is_null(), "getmntent should return null");
    }

    #[test]
    fn test_getmntent_r_returns_null() {
        let mut mntbuf = core::mem::MaybeUninit::<Mntent>::zeroed();
        let mut buf = [0u8; 256];
        let ret = getmntent_r(
            core::ptr::null_mut(),
            mntbuf.as_mut_ptr(),
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        assert!(ret.is_null(), "getmntent_r should return null");
    }

    #[test]
    fn test_endmntent_returns_one() {
        assert_eq!(endmntent(core::ptr::null_mut()), 1);
    }

    #[test]
    fn test_hasmntopt_returns_null() {
        let ret = hasmntopt(core::ptr::null(), b"rw\0".as_ptr());
        assert!(ret.is_null(), "hasmntopt should return null");
    }

    #[test]
    fn test_mntent_size() {
        let size = core::mem::size_of::<Mntent>();
        // 4 pointers + 2 i32 = 4*8 + 2*4 = 40 on 64-bit.
        assert!(size >= 40, "Mntent should be at least 40 bytes, got {size}");
    }

    // ------------------------------------------------------------------
    // personality
    // ------------------------------------------------------------------

    #[test]
    fn test_personality_query() {
        // 0xFFFFFFFF queries current personality.
        let ret = personality(0xFFFF_FFFF);
        assert_eq!(ret, 0, "Should return PER_LINUX (0)");
    }

    #[test]
    fn test_personality_set() {
        let ret = personality(0);
        assert_eq!(ret, 0, "Setting PER_LINUX should succeed");
    }

    // ------------------------------------------------------------------
    // ptrace
    // ------------------------------------------------------------------

    #[test]
    fn test_ptrace_enosys() {
        errno::set_errno(0);
        assert_eq!(ptrace(PTRACE_TRACEME, 0, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_ptrace_constants() {
        assert_eq!(PTRACE_TRACEME, 0);
        assert_eq!(PTRACE_PEEKTEXT, 1);
        assert_eq!(PTRACE_PEEKDATA, 2);
        assert_eq!(PTRACE_POKETEXT, 4);
        assert_eq!(PTRACE_POKEDATA, 5);
        assert_eq!(PTRACE_CONT, 7);
        assert_eq!(PTRACE_KILL, 8);
        assert_eq!(PTRACE_SINGLESTEP, 9);
        assert_eq!(PTRACE_ATTACH, 16);
        assert_eq!(PTRACE_DETACH, 17);
    }

    // ------------------------------------------------------------------
    // swapon / swapoff
    // ------------------------------------------------------------------

    #[test]
    fn test_swapon_enosys() {
        errno::set_errno(0);
        assert_eq!(swapon(b"/dev/sda2\0".as_ptr(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_swapoff_enosys() {
        errno::set_errno(0);
        assert_eq!(swapoff(b"/dev/sda2\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ------------------------------------------------------------------
    // klogctl
    // ------------------------------------------------------------------

    #[test]
    fn test_klogctl_enosys() {
        errno::set_errno(0);
        assert_eq!(klogctl(0, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ------------------------------------------------------------------
    // gethostid / sethostid
    // ------------------------------------------------------------------

    /// Reset HOSTID to the unset sentinel so per-test setup is consistent.
    fn reset_hostid_for_test() {
        use core::sync::atomic::Ordering;
        HOSTID.store(0, Ordering::Relaxed);
    }

    #[test]
    fn test_gethostid_unset_returns_hostname_hash() {
        reset_hostid_for_test();
        // Pin the hostname so the derived hash is deterministic.
        let name = b"hostid-test-A";
        assert_eq!(sethostname(name.as_ptr(), name.len()), 0);

        let id = gethostid();
        // Re-derive locally and compare.
        let expected = i64::from(fnv1a32(name) as i32);
        assert_eq!(id, expected);

        // Restore default hostname for other tests.
        let restore = b"localhost";
        let _ = sethostname(restore.as_ptr(), restore.len());
    }

    #[test]
    fn test_gethostid_stable_within_hostname() {
        reset_hostid_for_test();
        let name = b"hostid-test-B";
        assert_eq!(sethostname(name.as_ptr(), name.len()), 0);

        let a = gethostid();
        let b = gethostid();
        assert_eq!(a, b, "gethostid must be stable while hostname is unchanged");

        let restore = b"localhost";
        let _ = sethostname(restore.as_ptr(), restore.len());
    }

    #[test]
    fn test_gethostid_changes_with_hostname() {
        reset_hostid_for_test();
        let n1 = b"hostid-test-C1";
        let _ = sethostname(n1.as_ptr(), n1.len());
        let id1 = gethostid();

        let n2 = b"hostid-test-C2-different";
        let _ = sethostname(n2.as_ptr(), n2.len());
        let id2 = gethostid();

        assert_ne!(id1, id2, "different hostnames must hash to different ids");

        let restore = b"localhost";
        let _ = sethostname(restore.as_ptr(), restore.len());
    }

    #[test]
    fn test_sethostid_then_gethostid_roundtrip() {
        reset_hostid_for_test();
        assert_eq!(sethostid(0x1234_5678), 0);
        assert_eq!(gethostid(), 0x1234_5678);

        // Negative values (high bit set in 32-bit) sign-extend through i32→i64.
        assert_eq!(sethostid(0xFFFF_FFFF_u32 as i64), 0);
        assert_eq!(gethostid(), -1_i64);

        reset_hostid_for_test();
    }

    #[test]
    fn test_sethostid_truncates_to_32_bits() {
        reset_hostid_for_test();
        // Pass a value with bits set above the i32 range.
        assert_eq!(sethostid(0x1_0000_0001), 0);
        // The high bit (bit 32 in u64) is dropped; low 32 bits sign-extended.
        assert_eq!(gethostid(), 0x0000_0001);

        reset_hostid_for_test();
    }

    #[test]
    fn test_sethostid_zero_falls_back_to_hostname() {
        reset_hostid_for_test();
        let name = b"hostid-test-D";
        let _ = sethostname(name.as_ptr(), name.len());
        let derived = gethostid();

        // sethostid(0) must not turn off gethostid — it should keep
        // returning the hostname-derived value.
        assert_eq!(sethostid(0), 0);
        assert_eq!(gethostid(), derived);

        reset_hostid_for_test();
        let restore = b"localhost";
        let _ = sethostname(restore.as_ptr(), restore.len());
    }

    #[test]
    fn test_fnv1a32_known_vector() {
        // FNV-1a 32-bit canonical test vectors from the reference impl.
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
    }

    // ------------------------------------------------------------------
    // daemon
    // ------------------------------------------------------------------

    #[test]
    fn test_daemon_nochdir_noclose() {
        // daemon(1, 1) skips both chdir and close — essentially a no-op
        // except for setsid.
        let ret = daemon(1, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_daemon_noclose_only() {
        // daemon(0, 1) attempts chdir("/") which may succeed or fail
        // on the test host.  Either way, should not crash.
        let _ret = daemon(0, 1);
    }

    // ------------------------------------------------------------------
    // issetugid
    // ------------------------------------------------------------------

    #[test]
    fn test_issetugid_always_zero() {
        // Single-user OS: process never runs with elevated privileges.
        assert_eq!(issetugid(), 0);
    }

    #[test]
    fn test_issetugid_consistent() {
        // Multiple calls should return the same value.
        let a = issetugid();
        let b = issetugid();
        assert_eq!(a, b);
    }

    // ------------------------------------------------------------------
    // get_nprocs / get_nprocs_conf
    // ------------------------------------------------------------------

    #[test]
    fn test_get_nprocs_positive() {
        let n = get_nprocs();
        assert!(n >= 1, "get_nprocs should return at least 1, got {n}");
    }

    #[test]
    fn test_get_nprocs_conf_positive() {
        let n = get_nprocs_conf();
        assert!(n >= 1, "get_nprocs_conf should return at least 1, got {n}");
    }

    #[test]
    fn test_get_nprocs_le_conf() {
        // Online CPUs ≤ configured CPUs.
        let onln = get_nprocs();
        let conf = get_nprocs_conf();
        assert!(onln <= conf,
                "online ({onln}) should be ≤ configured ({conf})");
    }

    // ------------------------------------------------------------------
    // get_phys_pages / get_avphys_pages
    // ------------------------------------------------------------------

    #[test]
    fn test_get_phys_pages_positive() {
        let n = get_phys_pages();
        assert!(n > 0, "get_phys_pages should return > 0, got {n}");
    }

    #[test]
    fn test_get_avphys_pages_positive() {
        let n = get_avphys_pages();
        assert!(n > 0, "get_avphys_pages should return > 0, got {n}");
    }

    #[test]
    fn test_get_avphys_pages_le_phys() {
        // Available pages ≤ total physical pages.
        let avail = get_avphys_pages();
        let total = get_phys_pages();
        assert!(avail <= total,
                "available ({avail}) should be ≤ total ({total})");
    }

    // ------------------------------------------------------------------
    // futimesat
    // ------------------------------------------------------------------

    #[test]
    fn test_futimesat_null_path() {
        // null path → delegates to utimes which handles null.
        let _ret = futimesat(
            crate::file::AT_FDCWD,
            core::ptr::null(),
            core::ptr::null(),
        );
        // Just verify no crash.
    }

    #[test]
    fn test_futimesat_at_fdcwd() {
        // AT_FDCWD + relative path → delegates to utimes.
        let _ret = futimesat(
            crate::file::AT_FDCWD,
            b"/nonexistent\0".as_ptr(),
            core::ptr::null(),
        );
    }

    // ------------------------------------------------------------------
    // tmpnam_r
    // ------------------------------------------------------------------

    #[test]
    fn test_tmpnam_r_null() {
        let ret = unsafe { tmpnam_r(core::ptr::null_mut()) };
        assert!(ret.is_null(), "tmpnam_r(null) should return null");
    }

    #[test]
    fn test_tmpnam_r_with_buffer() {
        // Provide a buffer — tmpnam_r should populate it.
        let mut buf = [0u8; 64];
        let ret = unsafe { tmpnam_r(buf.as_mut_ptr()) };
        // On our OS, tmpnam generates /tmp/tmp_XXXXXX.
        // On test host it may succeed or fail; just verify no crash.
        let _ = ret;
    }

    // ------------------------------------------------------------------
    // get_current_dir_name
    // ------------------------------------------------------------------

    #[test]
    fn test_get_current_dir_name_returns_something() {
        // get_current_dir_name allocates a string with the CWD.
        // On test host, CWD is initialized to "/" or the actual cwd.
        let ptr = get_current_dir_name();
        // May return null if getcwd fails on test host.
        if !ptr.is_null() {
            // Should be a non-empty string.
            let first = unsafe { *ptr };
            assert_eq!(first, b'/', "CWD should start with '/'");
            // Free the allocation.
            unsafe { crate::malloc::free(ptr); }
        }
    }

    // -----------------------------------------------------------------
    // chroot / swapon / swapoff / klogctl — argument validation (Phase 61)
    // -----------------------------------------------------------------

    // ---- chroot ----

    #[test]
    fn test_chroot_null_efault() {
        errno::set_errno(0);
        assert_eq!(chroot(core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_chroot_empty_enoent() {
        errno::set_errno(0);
        assert_eq!(chroot(b"\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_chroot_one_char_path_enosys() {
        errno::set_errno(0);
        assert_eq!(chroot(b"a\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_chroot_workflow_sandbox_pivot() {
        // A daemon chroots into /var/empty before dropping privileges.
        // Validation passes → ENOSYS, so the daemon's "namespace
        // already isolated" fallback kicks in.
        errno::set_errno(0);
        assert_eq!(chroot(b"/var/empty\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ------------------------------------------------------------------
    // Phase 166: chroot — CAP_SYS_CHROOT gate
    //
    // Linux `fs/open.c::sys_chroot` resolves the user pointer via
    // `user_path_at` first, then runs `inode_permission`, then
    // checks `ns_capable(current_user_ns(), CAP_SYS_CHROOT)`.  So
    // path-domain errors (EFAULT / ENOENT / ENAMETOOLONG) beat
    // EPERM, but EPERM beats any later error including the
    // ENOSYS our stub surfaces.  Pre-Phase-166 the cap check was
    // missing entirely — an unprivileged caller saw ENOSYS for a
    // clean path instead of the EPERM Linux returns.
    // ------------------------------------------------------------------

    mod chroot_cap_phase166 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 77 / 164 / 165.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_chroot() {
            use crate::sys_capability::CAP_SYS_CHROOT;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_CHROOT < 32 {
                (lo & !(1u32 << CAP_SYS_CHROOT), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_CHROOT - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_CHROOT");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_CHROOT));
        }

        // -- Per-error-class --------------------------------------------------

        /// A clean call from an unprivileged process must surface
        /// EPERM, not the ENOSYS the stub used to return.
        #[test]
        fn test_chroot_phase166_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(b"/var/empty\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// A single-character path also reaches EPERM under no cap.
        #[test]
        fn test_chroot_phase166_one_char_path_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(b"a\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix --------------------------------------------------

        /// Linux: `user_path_at` runs before the cap check, so a
        /// NULL pointer yields EFAULT even without CAP_SYS_CHROOT.
        #[test]
        fn test_chroot_phase166_efault_beats_eperm_null_path() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        /// Same for an empty path — ENOENT wins because path
        /// resolution runs first.
        #[test]
        fn test_chroot_phase166_enoent_beats_eperm_empty_path() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(b"\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::ENOENT);
        }

        // -- Workflow ---------------------------------------------------------

        /// Workflow: an OpenSSH `sshd` worker (no CAP_SYS_CHROOT
        /// after sandbox setup) attempts a defensive chroot into
        /// `/var/empty` — Linux returns EPERM; we now do too.
        #[test]
        fn test_chroot_phase166_workflow_sshd_sandbox_attempt() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(b"/var/empty\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Workflow: a container runtime (think runc) that already
        /// pivoted root and dropped CAP_SYS_CHROOT calls chroot to
        /// double-isolate — must still see EPERM.
        #[test]
        fn test_chroot_phase166_workflow_runc_post_pivot_chroot() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(
                chroot(b"/run/containerd/rootfs\0".as_ptr()),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy-caller -----------------------------------------------------

        /// Buggy caller: a program that dropped caps then accidentally
        /// passed a NULL path — the EFAULT they get back doesn't leak
        /// the cap state (NULL beats EPERM regardless of caps).
        #[test]
        fn test_chroot_phase166_buggy_caller_null_under_no_cap() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        // -- Recovery ---------------------------------------------------------

        /// After EPERM, restoring CAP_SYS_CHROOT lets the next call
        /// reach ENOSYS for a clean path.
        #[test]
        fn test_chroot_phase166_recovery_after_eperm_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(b"/var/empty\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            drop(_g);
            errno::set_errno(0);
            assert_eq!(chroot(b"/var/empty\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- No-side-effect ---------------------------------------------------

        /// Calling chroot under no cap repeatedly never poisons the
        /// cap-restoration path — three back-to-back EPERMs then a
        /// guard-drop and a clean ENOSYS.
        #[test]
        fn test_chroot_phase166_repeated_eperm_does_not_poison_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            for _ in 0..3 {
                errno::set_errno(0);
                assert_eq!(chroot(b"/sandbox\0".as_ptr()), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
            drop(_g);
            errno::set_errno(0);
            assert_eq!(chroot(b"/sandbox\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Sentinel ---------------------------------------------------------

        /// Pre-Phase-166 chroot silently skipped the cap check; an
        /// unprivileged caller saw ENOSYS for a clean path.  This
        /// sentinel locks the new contract — explicit `assert_ne!`
        /// on ENOSYS so the failure message names the missing cap
        /// gate if the regression returns.
        #[test]
        fn test_chroot_phase166_no_longer_silently_skips_cap_check() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            errno::set_errno(0);
            assert_eq!(chroot(b"/srv\0".as_ptr()), -1);
            assert_ne!(errno::get_errno(), errno::ENOSYS,
                "Pre-Phase-166: unprivileged caller saw ENOSYS — \
                 CAP_SYS_CHROOT check missing.");
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Cross-checks -----------------------------------------------------

        /// Regression: default-cap caller still reaches ENOSYS for a
        /// clean call.  Phase 166 must not over-gate.
        #[test]
        fn test_chroot_phase166_capable_default_still_reaches_enosys() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_CHROOT
            ));
            errno::set_errno(0);
            assert_eq!(chroot(b"/var/empty\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Regression: default-cap caller still reaches EFAULT for
        /// a NULL pointer.
        #[test]
        fn test_chroot_phase166_capable_default_still_reaches_efault() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_CHROOT
            ));
            errno::set_errno(0);
            assert_eq!(chroot(core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        /// Regression: default-cap caller still reaches ENOENT for
        /// an empty string.
        #[test]
        fn test_chroot_phase166_capable_default_still_reaches_enoent() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_CHROOT
            ));
            errno::set_errno(0);
            assert_eq!(chroot(b"\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::ENOENT);
        }

        /// Dropping CAP_SYS_CHROOT must not affect other caps —
        /// only the chroot path is gated.  Verifies via swapon (which
        /// gates on CAP_SYS_ADMIN under Phase 164) that the broader
        /// cap state is intact.
        #[test]
        fn test_chroot_phase166_drop_chroot_does_not_affect_sys_admin() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_chroot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            // swapon under CAP_SYS_ADMIN reaches ENOSYS (its own
            // Phase 164 EPERM gate doesn't trip).
            errno::set_errno(0);
            assert_eq!(swapon(b"/swap\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }
    }

    // ---- swapon ----

    #[test]
    fn test_swap_flag_constants() {
        assert_eq!(SWAP_FLAG_PREFER, 0x8000);
        assert_eq!(SWAP_FLAG_DISCARD, 0x1_0000);
        assert_eq!(SWAP_FLAG_PRIO_MASK, 0x7FFF);
        // Priority mask must not overlap any flag bit.
        assert_eq!(SWAP_FLAG_PRIO_MASK & SWAP_FLAG_PREFER, 0);
        assert_eq!(SWAP_FLAG_PRIO_MASK & SWAP_FLAG_DISCARD, 0);
    }

    #[test]
    fn test_swapon_null_efault() {
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_swapon_empty_enoent() {
        errno::set_errno(0);
        assert_eq!(swapon(b"\0".as_ptr(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_swapon_unknown_flag_einval() {
        // Bit 19 is outside every defined swap flag.
        errno::set_errno(0);
        assert_eq!(swapon(b"/swap\0".as_ptr(), 0x80_0000), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_swapon_negative_flag_einval() {
        errno::set_errno(0);
        assert_eq!(swapon(b"/swap\0".as_ptr(), i32::MIN), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_swapon_flags_checked_before_path() {
        // Phase 122: Linux's sys_swapon checks `swap_flags &
        // ~SWAP_FLAGS_VALID` before calling getname, so a bad flag bit
        // beats EFAULT/ENOENT.  NULL path + bad flags → EINVAL.
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), 0x80_0000), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // --- Phase 122: validation order matches Linux sys_swapon ---

    /// Phase 122: empty path + bad flag bit — flag check fires first
    /// → EINVAL (not ENOENT).
    #[test]
    fn test_swapon_phase122_empty_bad_flag_einval() {
        errno::set_errno(0);
        assert_eq!(swapon(b"\0".as_ptr(), 0x80_0000), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Phase 122: NULL pointer + sign-bit flag (i32::MIN) — EINVAL
    /// from the mask check, never reaches the NULL deref check.
    #[test]
    fn test_swapon_phase122_null_i32_min_einval() {
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), i32::MIN), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Phase 122: NULL pointer with *clean* flags (PREFER + priority)
    /// — flag check passes, NULL caught next → EFAULT.
    #[test]
    fn test_swapon_phase122_null_clean_flags_efault() {
        errno::set_errno(0);
        let flags = SWAP_FLAG_PREFER | 3;
        assert_eq!(swapon(core::ptr::null(), flags), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// Phase 122: empty path with every valid flag bit set — flag
    /// check passes → empty path caught → ENOENT.
    #[test]
    fn test_swapon_phase122_empty_all_valid_flags_enoent() {
        errno::set_errno(0);
        assert_eq!(swapon(b"\0".as_ptr(), SWAP_FLAGS_VALID), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    /// Phase 122: NULL + every valid flag → EFAULT (flag pool clean,
    /// NULL fires).
    #[test]
    fn test_swapon_phase122_null_all_valid_flags_efault() {
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), SWAP_FLAGS_VALID), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// Phase 122: flags == 0 + NULL — historical no-flag swapon
    /// invocation (pre-DISCARD kernels).  Flag check trivially
    /// passes; NULL → EFAULT.
    #[test]
    fn test_swapon_phase122_zero_flags_null_efault() {
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// Phase 122: bit 16 just above the priority mask range — picks
    /// an *unknown* bit close to the legal flag region, confirming
    /// the mask test is exact, not "fuzzy".
    #[test]
    fn test_swapon_phase122_bit16_unknown_einval() {
        errno::set_errno(0);
        // SWAP_FLAG_PRIO_MASK is 0x7FFF; SWAP_FLAG_PREFER is 0x8000;
        // SWAP_FLAG_DISCARD = 0x10000.  Bit 17 (0x20000) is unknown
        // unless DISCARD_ONCE/DISCARD_PAGES cover it — check below.
        let unknown = 0x10_0000_i32;
        assert!(unknown & SWAP_FLAGS_VALID == 0,
            "test premise wrong: 0x100000 must be outside SWAP_FLAGS_VALID");
        assert_eq!(swapon(b"/swap\0".as_ptr(), unknown), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Phase 122: every unknown high bit ORed with every legal bit
    /// → EINVAL (mask rejects, regardless of legal bits set).
    #[test]
    fn test_swapon_phase122_legal_plus_unknown_einval() {
        errno::set_errno(0);
        let flags = SWAP_FLAGS_VALID | 0x4000_0000;
        assert_eq!(swapon(b"/swap\0".as_ptr(), flags), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Phase 122: errno recovery — a successful (modulo ENOSYS) call
    /// after an EINVAL cleanly overwrites errno.
    #[test]
    fn test_swapon_phase122_recovery_after_einval() {
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), 0x80_0000), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(swapon(b"/swap\0".as_ptr(), SWAP_FLAG_DISCARD), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    /// Phase 122: errno recovery in reverse — EFAULT followed by
    /// EINVAL.  Confirms each call overwrites cleanly.
    #[test]
    fn test_swapon_phase122_recovery_efault_then_einval() {
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(swapon(b"/swap\0".as_ptr(), 0x80_0000), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Phase 122 workflow: util-linux `swapon -v --priority 7
    /// /dev/sda2` resolves to `swapon("/dev/sda2", PREFER | 7)`.
    /// Must reach ENOSYS so the user sees "Function not implemented"
    /// rather than a wrong-args misdirection.
    #[test]
    fn test_swapon_phase122_workflow_util_linux_prio_7() {
        errno::set_errno(0);
        let path = b"/dev/sda2\0";
        let flags = SWAP_FLAG_PREFER | 7;
        assert_eq!(swapon(path.as_ptr(), flags), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    /// Phase 122 workflow: systemd-swap-on probe.  systemd may call
    /// `swapon(NULL, 0)` (with errno checking) as a syscall-presence
    /// probe.  Flag check passes (0 is legal) → NULL → EFAULT,
    /// confirming the syscall is wired up.  An EINVAL here would
    /// falsely suggest a missing syscall.
    #[test]
    fn test_swapon_phase122_workflow_systemd_probe() {
        errno::set_errno(0);
        assert_eq!(swapon(core::ptr::null(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// Phase 122 buggy-caller: a shell script computes
    /// `swap_flags=$((SWAP_FLAG_PREFER | priority))` but `priority`
    /// was read as a signed int and overflowed to `-1`.  Bitwise OR
    /// of -1 with anything is all-bits-set → unknown bits → EINVAL.
    #[test]
    fn test_swapon_phase122_buggy_caller_neg1_prio_einval() {
        errno::set_errno(0);
        let path = b"/dev/sda2\0";
        let flags = SWAP_FLAG_PREFER | (-1_i32);
        // -1 | anything == -1 == all bits set
        assert_eq!(flags, -1);
        assert_eq!(swapon(path.as_ptr(), flags), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_swapon_priority_only_reaches_enosys() {
        // PREFER + priority bits are a valid combination.
        let flags = SWAP_FLAG_PREFER | 5;
        errno::set_errno(0);
        assert_eq!(swapon(b"/swap\0".as_ptr(), flags), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_swapon_discard_reaches_enosys() {
        errno::set_errno(0);
        assert_eq!(swapon(b"/swap\0".as_ptr(), SWAP_FLAG_DISCARD), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_swapon_all_valid_flags_reaches_enosys() {
        errno::set_errno(0);
        assert_eq!(swapon(b"/swap\0".as_ptr(), SWAP_FLAGS_VALID), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- swapoff ----

    #[test]
    fn test_swapoff_null_efault() {
        errno::set_errno(0);
        assert_eq!(swapoff(core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_swapoff_empty_enoent() {
        errno::set_errno(0);
        assert_eq!(swapoff(b"\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    // ------------------------------------------------------------------
    // Phase 164: swapon / swapoff — CAP_SYS_ADMIN gate
    //
    // Linux `mm/swapfile.c::sys_swapon` and `sys_swapoff` perform a
    // `capable(CAP_SYS_ADMIN)` check at the very top of their syscall
    // prologue.  An unprivileged caller therefore gets EPERM *before*
    // the kernel inspects `swap_flags`, `getname()`, or any other
    // argument — that's how Linux avoids leaking whether the path or
    // flags would otherwise have been valid.  The pre-Phase-164 stubs
    // skipped this check, so a stripped-cap process could still drive
    // EINVAL/EFAULT/ENOENT, exposing the argument-validation lattice
    // it shouldn't be allowed to probe.
    // ------------------------------------------------------------------

    mod swap_cap_phase164 {
        use super::*;

        /// Snapshot the effective-cap bitset on construction; restore
        /// it on drop so EPERM tests don't bleed into the cooperating
        /// default tests that follow.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_admin() {
            use crate::sys_capability::CAP_SYS_ADMIN;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_ADMIN < 32 {
                (lo & !(1u32 << CAP_SYS_ADMIN), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_ADMIN - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_ADMIN");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_ADMIN));
        }

        // -- Per-error-class --------------------------------------------------

        /// Bare swapon under a stripped-cap caller must fail with
        /// EPERM, not ENOSYS.
        #[test]
        fn test_swapon_phase164_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(b"/dev/sda2\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Same for swapoff.
        #[test]
        fn test_swapoff_phase164_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapoff(b"/dev/sda2\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix: EPERM beats every later error -------------------

        /// EPERM must precede the bad-flag EINVAL — an unprivileged
        /// caller must not learn that they passed a bogus flag bit.
        #[test]
        fn test_swapon_phase164_eperm_beats_einval_bad_flag() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(b"/swap\0".as_ptr(), 0x80_0000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// EPERM must precede the NULL-pointer EFAULT.
        #[test]
        fn test_swapon_phase164_eperm_beats_efault_null_path() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(core::ptr::null(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// EPERM must precede the empty-path ENOENT.
        #[test]
        fn test_swapon_phase164_eperm_beats_enoent_empty_path() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(b"\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// EPERM must precede the combined bad-flag + NULL case.
        /// Without the cap check, this would have surfaced EINVAL
        /// (flag check happens before path).
        #[test]
        fn test_swapon_phase164_eperm_beats_einval_and_efault_combo() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(core::ptr::null(), i32::MIN), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// swapoff: EPERM beats EFAULT.
        #[test]
        fn test_swapoff_phase164_eperm_beats_efault_null_path() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapoff(core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// swapoff: EPERM beats ENOENT.
        #[test]
        fn test_swapoff_phase164_eperm_beats_enoent_empty_path() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapoff(b"\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Workflow ---------------------------------------------------------

        /// Workflow: a regular user (think `swapon -a` mis-invoked
        /// without sudo) should see EPERM no matter how realistic the
        /// path looks.
        #[test]
        fn test_swapon_phase164_workflow_unprivileged_user_attempts() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            let path = b"/dev/disk/by-uuid/12345\0";
            let flags = SWAP_FLAG_PREFER | 5;
            assert_eq!(swapon(path.as_ptr(), flags), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Workflow: systemd-shutdown drops CAP_SYS_ADMIN as part of
        /// the late-boot lockdown, then a stale unit tries to
        /// swapoff — Linux returns EPERM; we now do too.
        #[test]
        fn test_swapoff_phase164_workflow_systemd_drops_caps_then_swapoff() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapoff(b"/dev/sda3\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy-caller -----------------------------------------------------

        /// An unprivileged caller passing flag-bit garbage on the
        /// stack must still see EPERM, not the EINVAL the kernel would
        /// have surfaced for a privileged caller.
        #[test]
        fn test_swapon_phase164_buggy_caller_unprivileged_garbage_flags() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(b"/dev/sda2\0".as_ptr(), 0x0AC0_0000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery ---------------------------------------------------------

        /// After an EPERM, restoring CAP_SYS_ADMIN must let the next
        /// swapon proceed to the normal EINVAL path for a bad flag.
        #[test]
        fn test_swapon_phase164_recovery_after_eperm_reaches_einval() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(b"/swap\0".as_ptr(), 0x80_0000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Drop the guard explicitly to restore caps mid-test.
            drop(_g);
            errno::set_errno(0);
            assert_eq!(swapon(b"/swap\0".as_ptr(), 0x80_0000), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// After an EPERM, restoring CAP_SYS_ADMIN must let the next
        /// swapoff proceed to the normal ENOSYS path.
        #[test]
        fn test_swapoff_phase164_recovery_after_eperm_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapoff(b"/dev/sda3\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            drop(_g);
            errno::set_errno(0);
            assert_eq!(swapoff(b"/dev/sda3\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Sentinels (would fail under pre-Phase-164 behaviour) -------------

        /// Pre-Phase-164 swapon silently skipped the cap check; an
        /// unprivileged caller saw EINVAL for the bad flag instead of
        /// EPERM.  This sentinel locks the new contract.
        #[test]
        fn test_swapon_phase164_no_longer_silently_skips_cap_check() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(b"/swap\0".as_ptr(), 0x80_0000), -1);
            assert_ne!(errno::get_errno(), errno::EINVAL,
                "Pre-Phase-164: unprivileged caller saw EINVAL — \
                 capability check missing.");
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Sentinel for swapoff: pre-Phase-164 returned EFAULT for a
        /// null path even without CAP_SYS_ADMIN; now it returns EPERM.
        #[test]
        fn test_swapoff_phase164_no_longer_silently_skips_cap_check() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapoff(core::ptr::null()), -1);
            assert_ne!(errno::get_errno(), errno::EFAULT,
                "Pre-Phase-164: unprivileged caller saw EFAULT — \
                 capability check missing.");
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Cross-checks -----------------------------------------------------

        /// Both swapon and swapoff must share the same EPERM
        /// precedence: drop CAP_SYS_ADMIN once, both calls return
        /// EPERM in the same invocation.
        #[test]
        fn test_swapon_and_swapoff_phase164_share_eperm_precedence() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_admin();
            errno::set_errno(0);
            assert_eq!(swapon(b"/swap\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            errno::set_errno(0);
            assert_eq!(swapoff(b"/swap\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Regression: the default-cap path still surfaces EINVAL for
        /// a bad flag — Phase 164 must not over-gate the cap check.
        #[test]
        fn test_swapon_phase164_capable_default_still_reaches_einval() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            assert_eq!(swapon(b"/swap\0".as_ptr(), 0x80_0000), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// Regression: the default-cap swapoff path still surfaces
        /// EFAULT for a null path.
        #[test]
        fn test_swapoff_phase164_capable_default_still_reaches_efault() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            assert_eq!(swapoff(core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        /// Regression: default-cap swapon with all valid flags + good
        /// path still reaches ENOSYS.
        #[test]
        fn test_swapon_phase164_capable_default_valid_flags_reaches_enosys() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            assert_eq!(
                swapon(b"/dev/sda2\0".as_ptr(), SWAP_FLAGS_VALID),
                -1
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }
    }

    // ---- klogctl ----

    #[test]
    fn test_syslog_action_constants() {
        assert_eq!(SYSLOG_ACTION_CLOSE, 0);
        assert_eq!(SYSLOG_ACTION_OPEN, 1);
        assert_eq!(SYSLOG_ACTION_READ, 2);
        assert_eq!(SYSLOG_ACTION_READ_ALL, 3);
        assert_eq!(SYSLOG_ACTION_READ_CLEAR, 4);
        assert_eq!(SYSLOG_ACTION_CLEAR, 5);
        assert_eq!(SYSLOG_ACTION_CONSOLE_OFF, 6);
        assert_eq!(SYSLOG_ACTION_CONSOLE_ON, 7);
        assert_eq!(SYSLOG_ACTION_CONSOLE_LEVEL, 8);
        assert_eq!(SYSLOG_ACTION_SIZE_UNREAD, 9);
        assert_eq!(SYSLOG_ACTION_SIZE_BUFFER, 10);
        assert_eq!(SYSLOG_ACTION_MAX, 10);
    }

    #[test]
    fn test_klogctl_negative_cmd_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(-1, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_cmd_above_max_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_MAX + 1, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_cmd_way_above_max_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(100, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_read_null_buf_einval() {
        // Linux do_syslog: `if (!buf || len < 0) error = -EINVAL`.
        // NULL buf is EINVAL, *not* EFAULT.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), 16), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_read_all_null_buf_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ_ALL, core::ptr::null_mut(), 16), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_read_clear_null_buf_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ_CLEAR, core::ptr::null_mut(), 16), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_read_negative_len_einval() {
        let mut buf = [0u8; 16];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), -1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_console_level_zero_einval() {
        // Below SYSLOG_LOG_LEVEL_MIN.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_console_level_nine_einval() {
        // Above SYSLOG_LOG_LEVEL_MAX.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), 9), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_console_level_negative_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), -3), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_close_reaches_enosys() {
        // CLOSE takes no buf/len; passes through.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_CLOSE, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_klogctl_clear_reaches_enosys() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_CLEAR, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_klogctl_size_unread_reaches_enosys() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_SIZE_UNREAD, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_klogctl_size_buffer_reaches_enosys() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_SIZE_BUFFER, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_klogctl_read_valid_args_reaches_enosys() {
        let mut buf = [0u8; 64];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 64), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_klogctl_console_level_valid_reaches_enosys() {
        for level in SYSLOG_LOG_LEVEL_MIN..=SYSLOG_LOG_LEVEL_MAX {
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), level),
                -1,
                "level={level} should reach -1",
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS, "level={level}");
        }
    }

    #[test]
    fn test_klogctl_cmd_checked_before_buf() {
        // Bad cmd + NULL buf → EINVAL from cmd check.
        errno::set_errno(0);
        assert_eq!(klogctl(-5, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_read_null_buf_or_negative_len_both_einval() {
        // Linux folds `!buf || len < 0` into one EINVAL test, so either
        // condition (or both together) surfaces the same errno.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), -1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Real-world workflows ----

    #[test]
    fn test_workflow_dmesg_read_all() {
        // `dmesg` calls klogctl(SYSLOG_ACTION_READ_ALL, buf, sizeof(buf))
        // to dump the kernel log. Validates → ENOSYS, dmesg prints
        // "klogctl: Function not implemented" and exits cleanly.
        let mut buf = [0u8; 8192];
        errno::set_errno(0);
        assert_eq!(
            klogctl(SYSLOG_ACTION_READ_ALL, buf.as_mut_ptr(), buf.len() as i32),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // Phase 157 — `klogctl(READ*, valid_buf, 0)` returns 0, not ENOSYS.
    //
    // Linux's `do_syslog` (kernel/printk/printk.c):
    //
    //     if (!buf || len < 0)
    //         return -EINVAL;
    //     if (!len)
    //         return 0;
    //     if (!access_ok(buf, len))
    //         return -EFAULT;
    //
    // The `if (!len) return 0;` short-circuit fires **after** the EINVAL
    // guard but **before** access_ok and any backend dispatch.  A
    // zero-byte read is a no-op success regardless of whether the
    // kernel-log backend is implemented.  Our pre-Phase-157 stub fell
    // through to ENOSYS, diverging from glibc/musl-tested behaviour.
    // -----------------------------------------------------------------

    // -- Per-error-class --------------------------------------------------

    #[test]
    fn test_klogctl_read_zero_len_valid_buf_returns_zero_phase157() {
        let mut buf = [0u8; 64];
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0);
        assert_eq!(ret, 0, "zero-byte read must be a no-op success");
        // errno preserved (POSIX: success does not clear errno).
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_klogctl_read_all_zero_len_returns_zero_phase157() {
        let mut buf = [0u8; 64];
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ_ALL, buf.as_mut_ptr(), 0);
        assert_eq!(ret, 0);
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_klogctl_read_clear_zero_len_returns_zero_phase157() {
        let mut buf = [0u8; 64];
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ_CLEAR, buf.as_mut_ptr(), 0);
        assert_eq!(ret, 0);
        assert_eq!(errno::get_errno(), 0);
    }

    // -- Ordering matrix --------------------------------------------------

    /// EINVAL must still win when both `buf == NULL` and `len == 0` —
    /// the EINVAL guard precedes the `len == 0` shortcut in Linux's order.
    #[test]
    fn test_klogctl_read_null_buf_zero_len_still_einval_phase157() {
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// EINVAL also wins for NULL buf + len < 0.
    #[test]
    fn test_klogctl_read_null_buf_negative_len_still_einval_phase157() {
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), -1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Valid buf + len < 0 → EINVAL (negative len short-circuits the
    /// len == 0 shortcut).
    #[test]
    fn test_klogctl_read_valid_buf_negative_len_still_einval_phase157() {
        let mut buf = [0u8; 64];
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), -1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Bad cmd + zero len still hits the top-level cmd-EINVAL guard.
    #[test]
    fn test_klogctl_bad_cmd_zero_len_still_einval_phase157() {
        errno::set_errno(0);
        let mut buf = [0u8; 64];
        let ret = klogctl(-1, buf.as_mut_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Workflow ---------------------------------------------------------

    /// `len > 0` still hits the ENOSYS backend path — Phase 157 only
    /// affects len == 0.
    #[test]
    fn test_klogctl_read_nonzero_len_still_enosys_phase157() {
        let mut buf = [0u8; 64];
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    /// A program polling the log for "anything new" with a zero-byte
    /// probe (a real glibc idiom: `klogctl(2, NULL, 0)`-style probes
    /// don't compile because of EINVAL, but `klogctl(2, buf, 0)` does
    /// and is the recommended way to test for log-read capability).
    #[test]
    fn test_klogctl_zero_probe_workflow_phase157() {
        let mut buf = [0u8; 1];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0), 0);
        // The zero-byte probe must not leave errno latched at ENOSYS.
        assert_eq!(errno::get_errno(), 0);
    }

    // -- Buggy-caller -----------------------------------------------------

    /// Caller passes a `buf` they later check for "any data written" —
    /// Phase 157 must not touch the buffer on the zero-len path.
    #[test]
    fn test_klogctl_read_zero_len_does_not_touch_buf_phase157() {
        let mut buf = [0xAAu8; 32];
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0);
        assert_eq!(ret, 0);
        // Buffer untouched — every byte still 0xAA.
        for (i, &b) in buf.iter().enumerate() {
            assert_eq!(b, 0xAA, "buf[{i}] was modified");
        }
    }

    // -- Recovery / no-side-effect loop -----------------------------------

    /// Pre-seeded errno survives a zero-len success.
    #[test]
    fn test_klogctl_read_zero_len_does_not_set_errno_phase157() {
        let mut buf = [0u8; 8];
        errno::set_errno(errno::EIO);
        let ret = klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0);
        assert_eq!(ret, 0);
        // POSIX: errno is preserved across a successful call.
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    /// After the zero-len success, a follow-up nonzero call still goes
    /// to ENOSYS — no sticky state from the short-circuit.
    #[test]
    fn test_klogctl_recover_after_zero_len_success_phase157() {
        let mut buf = [0u8; 8];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0), 0);
        // Now request 4 bytes — backend missing → ENOSYS.
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 4);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- Sentinel ---------------------------------------------------------

    /// Explicit sentinel: pre-Phase-157 the zero-len read returned
    /// ENOSYS.  If a future regression reintroduces that, this fails.
    #[test]
    fn test_klogctl_read_zero_len_no_longer_enosys_phase157() {
        let mut buf = [0u8; 8];
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0);
        assert_ne!(
            ret, -1,
            "zero-len read must no longer return -1"
        );
        assert_ne!(
            errno::get_errno(),
            errno::ENOSYS,
            "zero-len read must no longer set ENOSYS"
        );
    }

    // -- Cross-checks -----------------------------------------------------

    /// CONSOLE_LEVEL with len == 0 must still be EINVAL (level 0 is
    /// outside [1, 8]).  Phase 157 doesn't touch this path.
    #[test]
    fn test_klogctl_console_level_zero_len_still_einval_phase157() {
        errno::set_errno(0);
        let ret = klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// CLOSE/OPEN/CLEAR/CONSOLE_OFF/CONSOLE_ON with len == 0 are still
    /// the ENOSYS-backend path — Phase 157 only short-circuits the
    /// READ family.
    #[test]
    fn test_klogctl_non_read_zero_len_still_enosys_phase157() {
        for cmd in [
            SYSLOG_ACTION_CLOSE,
            SYSLOG_ACTION_OPEN,
            SYSLOG_ACTION_CLEAR,
            SYSLOG_ACTION_CONSOLE_OFF,
            SYSLOG_ACTION_CONSOLE_ON,
            SYSLOG_ACTION_SIZE_UNREAD,
            SYSLOG_ACTION_SIZE_BUFFER,
        ] {
            errno::set_errno(0);
            let ret = klogctl(cmd, core::ptr::null_mut(), 0);
            assert_eq!(ret, -1, "cmd={cmd} must reach ENOSYS");
            assert_eq!(
                errno::get_errno(),
                errno::ENOSYS,
                "cmd={cmd} must set ENOSYS"
            );
        }
    }

    #[test]
    fn test_workflow_systemd_set_console_level() {
        // systemd lowers the console log level during early boot via
        // klogctl(8, NULL, 4). Validates → ENOSYS; systemd logs the
        // failure and proceeds with default verbosity.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), 4), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_workflow_mkswap_then_swapon() {
        // mkswap formats /dev/sda3 as swap; the installer then calls
        // swapon("/dev/sda3", 0). On our committed-memory OS this
        // returns ENOSYS and the installer continues without swap.
        errno::set_errno(0);
        assert_eq!(swapon(b"/dev/sda3\0".as_ptr(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- Real-world buggy callers ----

    #[test]
    fn test_workflow_buggy_swapon_garbage_flags() {
        // Caller forgot to zero `swapflags` before swapon(); stack
        // garbage shows up as unknown bits → EINVAL.  Without our
        // validation the kernel would have happily accepted them.
        errno::set_errno(0);
        assert_eq!(swapon(b"/dev/sda3\0".as_ptr(), 0x0AC0_0000), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_workflow_buggy_klogctl_typo_cmd() {
        // Caller meant SYSLOG_ACTION_READ (2) but typed 20.  EINVAL is
        // the correct diagnostic.
        errno::set_errno(0);
        assert_eq!(klogctl(20, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_workflow_buggy_chroot_empty_argv() {
        // A script passes "" from an unchecked argv[1] to chroot();
        // ENOENT is the correct (and informative) diagnostic.
        errno::set_errno(0);
        assert_eq!(chroot(b"\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    // ------------------------------------------------------------------
    // Phase 126: klogctl Linux-EINVAL parity for NULL buf in READ family
    // ------------------------------------------------------------------
    //
    // Linux's kernel/printk/printk.c::do_syslog folds `!buf || len < 0`
    // into a single `error = -EINVAL` test for SYSLOG_ACTION_READ,
    // READ_ALL, and READ_CLEAR.  Earlier phases mis-modelled NULL buf
    // as EFAULT; these tests pin down the corrected (Linux-matching)
    // behaviour.

    #[test]
    fn test_klogctl_phase126_read_null_buf_positive_len_einval() {
        // NULL buf + valid positive len — Linux returns EINVAL from the
        // combined `!buf || len < 0` test, before any access_ok check.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), 4096), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_read_all_null_buf_positive_len_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ_ALL, core::ptr::null_mut(), 4096), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_read_clear_null_buf_positive_len_einval() {
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ_CLEAR, core::ptr::null_mut(), 4096), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_read_null_buf_zero_len_einval() {
        // Linux's `if (!buf || len < 0)` fires before the `if (!len)`
        // shortcut, so NULL buf + len == 0 is still EINVAL — *not* the
        // zero-len success path.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_read_negative_len_with_valid_buf_einval() {
        // Non-NULL buf, but negative len — the `len < 0` half of the
        // combined check fires.  Same errno as NULL buf.
        let mut buf = [0u8; 32];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), -1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_read_all_negative_len_with_valid_buf_einval() {
        let mut buf = [0u8; 32];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ_ALL, buf.as_mut_ptr(), i32::MIN), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_read_null_and_negative_einval() {
        // Both halves of the combined check would fire; we only ever
        // surface one EINVAL regardless.
        errno::set_errno(0);
        assert_eq!(
            klogctl(SYSLOG_ACTION_READ_CLEAR, core::ptr::null_mut(), -100),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_cmd_beats_null_buf() {
        // Validation order: out-of-range cmd fires before any buf/len
        // check (Linux's `default:` arm in the switch is reached after
        // the unknown cmd falls through; no buf inspection occurs).
        errno::set_errno(0);
        assert_eq!(klogctl(-1, core::ptr::null_mut(), 16), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_valid_read_with_zero_len_reaches_enosys() {
        // Phase 126 originally chose to surface ENOSYS here, even though
        // Linux's `do_syslog` shortcuts `len == 0` to a 0-byte success
        // before touching the backend.  Phase 157 reversed that choice
        // and now matches Linux exactly — see
        // `test_klogctl_read_zero_len_valid_buf_returns_zero_phase157`.
        //
        // This test is retasked to assert the **post-Phase-157**
        // contract: non-NULL buf + len == 0 returns 0 (no errno set),
        // proving the EINVAL guard didn't catch the request and that
        // we no longer fall through to ENOSYS.
        let mut buf = [0u8; 16];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0), 0);
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_klogctl_phase126_console_level_with_null_buf_still_valid() {
        // CONSOLE_LEVEL never reads from buf — NULL is fine, only the
        // level (in `len`) is validated.  Confirms the buf NULL-check
        // only applies to the read family.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), 4), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_klogctl_phase126_workflow_systemd_journal_probe() {
        // systemd-journald probes the kernel log via
        // klogctl(SYSLOG_ACTION_READ_ALL, NULL, 0) to detect whether a
        // klog exists.  Pre-fix this returned EFAULT (misleading);
        // post-fix it returns EINVAL, matching what journald sees on
        // Linux when it inadvertently passes NULL.
        errno::set_errno(0);
        assert_eq!(
            klogctl(SYSLOG_ACTION_READ_ALL, core::ptr::null_mut(), 0),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_workflow_buggy_dmesg_uninit_buf_ptr() {
        // A C program reuses `char *p` from an earlier branch where it
        // was set to NULL but forgets to allocate before passing to
        // klogctl(READ_ALL, p, 8192).  Post-fix the caller sees EINVAL
        // ("bad argument") which is much more diagnostic than EFAULT
        // ("bad address") — the bug is in their argument, not their
        // memory map.
        errno::set_errno(0);
        assert_eq!(
            klogctl(SYSLOG_ACTION_READ_ALL, core::ptr::null_mut(), 8192),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_klogctl_phase126_recovery_after_null_buf_einval() {
        // Per-call errno: an EINVAL from a NULL buf doesn't poison a
        // subsequent well-formed call.
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), 16), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let mut buf = [0u8; 64];
        errno::set_errno(0);
        assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), buf.len() as i32), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ------------------------------------------------------------------
    // Phase 64: ptrace argument-domain validation
    // ------------------------------------------------------------------

    #[test]
    fn test_ptrace_extended_constants() {
        // New constants added in Phase 64.
        assert_eq!(PTRACE_PEEKUSER, 3);
        assert_eq!(PTRACE_POKEUSER, 6);
        assert_eq!(PTRACE_SYSCALL, 24);
        assert_eq!(PTRACE_SETOPTIONS, 0x4200);
        assert_eq!(PTRACE_GETEVENTMSG, 0x4201);
        assert_eq!(PTRACE_GETSIGINFO, 0x4202);
        assert_eq!(PTRACE_SETSIGINFO, 0x4203);
        assert_eq!(PTRACE_SEIZE, 0x4206);
        assert_eq!(PTRACE_INTERRUPT, 0x4207);
        assert_eq!(PTRACE_LISTEN, 0x4208);
    }

    #[test]
    fn test_ptrace_request_known_recognizes_all_constants() {
        for r in &[
            PTRACE_TRACEME, PTRACE_PEEKTEXT, PTRACE_PEEKDATA, PTRACE_PEEKUSER,
            PTRACE_POKETEXT, PTRACE_POKEDATA, PTRACE_POKEUSER,
            PTRACE_CONT, PTRACE_KILL, PTRACE_SINGLESTEP,
            PTRACE_ATTACH, PTRACE_DETACH, PTRACE_SYSCALL,
            PTRACE_SETOPTIONS, PTRACE_GETEVENTMSG,
            PTRACE_GETSIGINFO, PTRACE_SETSIGINFO,
            PTRACE_SEIZE, PTRACE_INTERRUPT, PTRACE_LISTEN,
        ] {
            assert!(ptrace_request_known(*r), "unknown known request {r}");
        }
    }

    #[test]
    fn test_ptrace_request_known_rejects_garbage() {
        // Values not in our recognised set.
        assert!(!ptrace_request_known(100));
        assert!(!ptrace_request_known(1_000_000));
        assert!(!ptrace_request_known(-1));
        assert!(!ptrace_request_known(i32::MAX));
        assert!(!ptrace_request_known(i32::MIN));
        // 10..15 — gap between SINGLESTEP(9) and ATTACH(16).
        assert!(!ptrace_request_known(10));
        assert!(!ptrace_request_known(15));
    }

    // --- unknown request → EIO ----------------------------------------

    #[test]
    fn test_ptrace_unknown_request_eio() {
        errno::set_errno(0);
        let ret = ptrace(100, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    #[test]
    fn test_ptrace_negative_request_eio() {
        errno::set_errno(0);
        let ret = ptrace(-5, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    #[test]
    fn test_ptrace_gap_request_eio() {
        // Codes 10..15 are gaps in the standard ptrace numbering.
        errno::set_errno(0);
        assert_eq!(ptrace(11, 1, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    // --- PTRACE_TRACEME ignores pid/addr/data --------------------------

    #[test]
    fn test_ptrace_traceme_ignores_pid() {
        // Even pid <= 0 must not turn into ESRCH for TRACEME.
        errno::set_errno(0);
        assert_eq!(ptrace(PTRACE_TRACEME, -42, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_ptrace_traceme_ignores_addr_data() {
        errno::set_errno(0);
        assert_eq!(
            ptrace(PTRACE_TRACEME, 0, 0xDEAD_BEEF, 0xCAFE_BABE),
            -1
        );
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // --- other requests: pid validation -------------------------------

    #[test]
    fn test_ptrace_attach_zero_pid_esrch() {
        errno::set_errno(0);
        let ret = ptrace(PTRACE_ATTACH, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_ptrace_attach_negative_pid_esrch() {
        errno::set_errno(0);
        let ret = ptrace(PTRACE_ATTACH, -1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_ptrace_detach_zero_pid_esrch() {
        errno::set_errno(0);
        let ret = ptrace(PTRACE_DETACH, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_ptrace_peektext_negative_pid_esrch() {
        errno::set_errno(0);
        let ret = ptrace(PTRACE_PEEKTEXT, -100, 0x1000, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_ptrace_cont_min_pid_esrch() {
        errno::set_errno(0);
        let ret = ptrace(PTRACE_CONT, i32::MIN, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    // --- other requests with positive pid: reach ENOSYS ----------------

    #[test]
    fn test_ptrace_attach_positive_pid_enosys() {
        // Valid request, positive pid — reaches the ENOSYS sentinel.
        errno::set_errno(0);
        let ret = ptrace(PTRACE_ATTACH, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_ptrace_peekdata_positive_pid_enosys() {
        errno::set_errno(0);
        let ret = ptrace(PTRACE_PEEKDATA, 42, 0x1000, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_ptrace_seize_positive_pid_enosys() {
        errno::set_errno(0);
        let ret = ptrace(PTRACE_SEIZE, 1234, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // --- ordering -----------------------------------------------------

    #[test]
    fn test_ptrace_request_check_before_pid_check() {
        // Unknown request AND bad pid — EIO wins.
        errno::set_errno(0);
        let ret = ptrace(999_999, -1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    #[test]
    fn test_ptrace_traceme_takes_precedence_over_pid_check() {
        // PTRACE_TRACEME is a known request and skips the pid check
        // entirely — even pid == 0 / negative must not produce ESRCH.
        errno::set_errno(0);
        let ret = ptrace(PTRACE_TRACEME, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // --- workflows + buggy callers ------------------------------------

    #[test]
    fn test_workflow_strace_style_attach_then_detach() {
        // strace-like flow: ATTACH to pid, then DETACH.  Both must
        // reach the ENOSYS sentinel on a valid pid (we don't actually
        // attach, but the syscall shape must be correct).
        errno::set_errno(0);
        assert_eq!(ptrace(PTRACE_ATTACH, 100, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        errno::set_errno(0);
        assert_eq!(ptrace(PTRACE_DETACH, 100, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_workflow_gdb_step_loop() {
        // gdb step loop: SETOPTIONS once, then alternate SYSCALL +
        // GETSIGINFO.  All known requests must reach ENOSYS.
        errno::set_errno(0);
        assert_eq!(ptrace(PTRACE_SETOPTIONS, 50, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        errno::set_errno(0);
        assert_eq!(ptrace(PTRACE_SYSCALL, 50, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        errno::set_errno(0);
        assert_eq!(ptrace(PTRACE_GETSIGINFO, 50, 0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_buggy_caller_ptrace_signed_unsigned_confusion() {
        // Some debuggers cast a u32 PTRACE_* macro to i32 and pass
        // it through; the historical PTRACE_GETREGS (12) and similar
        // codes are NOT in our recognised set (we only have the
        // generic POSIX subset).  EIO is the right diagnostic.
        errno::set_errno(0);
        let ret = ptrace(12, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    #[test]
    fn test_buggy_caller_ptrace_kill_self_with_zero_pid() {
        // Caller passes pid=0 to PTRACE_KILL meaning "current
        // process" — that's wait(2) semantics, not ptrace(2)
        // semantics.  ptrace rejects with ESRCH.
        errno::set_errno(0);
        let ret = ptrace(PTRACE_KILL, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    // =====================================================================
    // Phase 200 — CAP_SYS_PTRACE gate on ptrace (non-TRACEME requests)
    //
    // Linux's ptrace_may_access() checks CAP_SYS_PTRACE after finding the
    // target task.  In our stub, the cap gate runs after the pid <= 0 →
    // ESRCH check.  PTRACE_TRACEME bypasses the gate (tracing yourself
    // doesn't need ptrace capability).
    // =====================================================================

    // -- cap helpers (scoped to this phase) --------------------------------

    mod phase200_cap_helpers {
        pub(super) struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            pub(super) fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        pub(super) fn drop_cap_sys_ptrace() {
            let cap = crate::sys_capability::CAP_SYS_PTRACE;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed dropping cap");
            assert!(!crate::sys_capability::has_capability(cap));
        }
    }

    // -- per-error class: cap held ----------------------------------------

    /// With CAP_SYS_PTRACE held (default), valid ptrace requests reach
    /// ENOSYS (unchanged from pre-Phase 200 behavior).
    #[test]
    fn test_phase200_ptrace_attach_with_cap_reaches_enosys() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SYS_PTRACE,
        ));
        errno::set_errno(0);
        let ret = ptrace(PTRACE_ATTACH, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- per-error class: cap dropped → EPERM -----------------------------

    /// Without CAP_SYS_PTRACE, PTRACE_ATTACH → EPERM.
    #[test]
    fn test_phase200_ptrace_attach_no_cap_eperm() {
        let _g = phase200_cap_helpers::CapGuard::snapshot();
        phase200_cap_helpers::drop_cap_sys_ptrace();
        errno::set_errno(0);
        let ret = ptrace(PTRACE_ATTACH, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// Without CAP_SYS_PTRACE, PTRACE_SEIZE → EPERM.
    #[test]
    fn test_phase200_ptrace_seize_no_cap_eperm() {
        let _g = phase200_cap_helpers::CapGuard::snapshot();
        phase200_cap_helpers::drop_cap_sys_ptrace();
        errno::set_errno(0);
        let ret = ptrace(PTRACE_SEIZE, 42, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// Without CAP_SYS_PTRACE, PTRACE_PEEKTEXT → EPERM.
    #[test]
    fn test_phase200_ptrace_peektext_no_cap_eperm() {
        let _g = phase200_cap_helpers::CapGuard::snapshot();
        phase200_cap_helpers::drop_cap_sys_ptrace();
        errno::set_errno(0);
        let ret = ptrace(PTRACE_PEEKTEXT, 100, 0x1000, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    // -- PTRACE_TRACEME bypasses the cap gate -----------------------------

    /// PTRACE_TRACEME does not require CAP_SYS_PTRACE — tracing
    /// yourself is always allowed (subject to "already traced" check,
    /// which we stub as ENOSYS).
    #[test]
    fn test_phase200_ptrace_traceme_no_cap_still_enosys() {
        let _g = phase200_cap_helpers::CapGuard::snapshot();
        phase200_cap_helpers::drop_cap_sys_ptrace();
        errno::set_errno(0);
        let ret = ptrace(PTRACE_TRACEME, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::ENOSYS,
            "TRACEME must bypass CAP_SYS_PTRACE gate"
        );
    }

    // -- ordering: EIO before EPERM, ESRCH before EPERM -------------------

    /// Unknown request + no cap → EIO (request check runs first).
    #[test]
    fn test_phase200_ptrace_unknown_request_eio_before_eperm() {
        let _g = phase200_cap_helpers::CapGuard::snapshot();
        phase200_cap_helpers::drop_cap_sys_ptrace();
        errno::set_errno(0);
        let ret = ptrace(9999, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EIO,
            "EIO for unknown request must precede EPERM"
        );
    }

    /// pid <= 0 + no cap → ESRCH (pid check runs before cap check).
    #[test]
    fn test_phase200_ptrace_bad_pid_esrch_before_eperm() {
        let _g = phase200_cap_helpers::CapGuard::snapshot();
        phase200_cap_helpers::drop_cap_sys_ptrace();
        errno::set_errno(0);
        let ret = ptrace(PTRACE_ATTACH, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::ESRCH,
            "ESRCH for bad pid must precede EPERM"
        );
    }

    // -- restoration: CapGuard drop re-enables ptrace ---------------------

    /// After restoring CAP_SYS_PTRACE, valid ptrace reaches ENOSYS again.
    #[test]
    fn test_phase200_ptrace_cap_restore_re_enables() {
        {
            let _g = phase200_cap_helpers::CapGuard::snapshot();
            phase200_cap_helpers::drop_cap_sys_ptrace();
            errno::set_errno(0);
            let ret = ptrace(PTRACE_ATTACH, 1, 0, 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EPERM, "must fail without cap");
        }
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SYS_PTRACE,
        ));
        errno::set_errno(0);
        let ret = ptrace(PTRACE_ATTACH, 1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS, "must pass after restore");
    }

    // =====================================================================
    // Phase 73 — fpathconf fd validation
    //
    // POSIX fpathconf operates on an open fd.  Linux's prologue rejects
    // negative or unopen fds with -1/EBADF before doing any work.  Our
    // pathconf table-driven body is shared between fpathconf and pathconf,
    // so we only need to verify the fd-validation gate here.
    // =====================================================================

    // ---- Per-error class: bad fd ----

    #[test]
    fn test_fpathconf_negative_fd_returns_ebadf() {
        errno::set_errno(0);
        assert_eq!(fpathconf(-1, _PC_NAME_MAX), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fpathconf_large_negative_fd_returns_ebadf() {
        errno::set_errno(0);
        assert_eq!(fpathconf(i32::MIN, _PC_NAME_MAX), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fpathconf_unopen_fd_returns_ebadf() {
        let probe: i32 = 0x4000_0050;
        let _ = crate::fdtable::close_fd(probe);
        errno::set_errno(0);
        assert_eq!(fpathconf(probe, _PC_NAME_MAX), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ---- Open fd: delegates to pathconf table ----

    #[test]
    fn test_fpathconf_open_fd_path_max() {
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0)
            .expect("alloc_fd File failed");
        // _PC_PATH_MAX must match what pathconf returns for the same name.
        let via_path = pathconf(core::ptr::null(), _PC_PATH_MAX);
        let via_fd = fpathconf(fd, _PC_PATH_MAX);
        assert_eq!(via_fd, via_path);
        let _ = crate::fdtable::close_fd(fd);
    }

    #[test]
    fn test_fpathconf_open_fd_name_max() {
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0)
            .expect("alloc_fd File failed");
        let via_path = pathconf(core::ptr::null(), _PC_NAME_MAX);
        let via_fd = fpathconf(fd, _PC_NAME_MAX);
        assert_eq!(via_fd, via_path);
        let _ = crate::fdtable::close_fd(fd);
    }

    // ---- Validation ordering: bad fd beats bad name ----

    #[test]
    fn test_fpathconf_bad_fd_beats_bad_name() {
        // Even though `name` is unknown (would yield -1 with EINVAL via
        // pathconf), the fd check fires first → EBADF, not EINVAL.
        errno::set_errno(0);
        assert_eq!(fpathconf(-1, 99999), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fpathconf_unopen_fd_beats_bad_name() {
        let probe: i32 = 0x4000_0051;
        let _ = crate::fdtable::close_fd(probe);
        errno::set_errno(0);
        assert_eq!(fpathconf(probe, 99999), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ---- Buggy-caller patterns ----

    #[test]
    fn test_fpathconf_buggy_uninit_fd_returns_ebadf() {
        // Caller leaves fd uninitialised — happens to be -1.
        let mut fd: i32 = -1;
        fd = fd.wrapping_add(0);
        errno::set_errno(0);
        assert_eq!(fpathconf(fd, _PC_NAME_MAX), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fpathconf_buggy_closed_fd_after_close() {
        // Caller closes the fd, then queries fpathconf on the stale
        // handle.  Must fail with EBADF.
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0)
            .expect("alloc_fd File failed");
        let _ = crate::fdtable::close_fd(fd);
        errno::set_errno(0);
        assert_eq!(fpathconf(fd, _PC_NAME_MAX), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ======================================================================
    // Phase 172 — klogctl CAP_SYSLOG gate
    //
    // Linux `kernel/printk/printk.c::check_syslog_permissions` rejects most
    // syslog actions with -EPERM when the caller lacks CAP_SYSLOG (under
    // the default `dmesg_restrict=0`).  CLOSE, OPEN, SIZE_BUFFER are
    // unconditionally allowed (they're no-ops on Linux).  READ_ALL and
    // SIZE_UNREAD are likewise allowed without the cap.  Everything else
    // — READ, READ_CLEAR, CLEAR, CONSOLE_OFF, CONSOLE_ON, CONSOLE_LEVEL —
    // requires CAP_SYSLOG → EPERM.
    //
    // These tests must run with `--test-threads=1` because they mutate
    // the process-wide capability state.  Each test snapshots the
    // effective set on entry and restores it on drop.
    // ======================================================================

    mod klogctl_cap_phase172 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 168 – 171.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_syslog() {
            use crate::sys_capability::CAP_SYSLOG;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYSLOG < 32 {
                (lo & !(1u32 << CAP_SYSLOG), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYSLOG - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYSLOG");
            assert!(!crate::sys_capability::has_capability(CAP_SYSLOG));
        }

        // -- Per-error-class ---------------------------------------------

        /// READ without CAP_SYSLOG → EPERM (after EINVAL guards pass).
        #[test]
        fn test_klogctl_phase172_read_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            let mut buf = [0u8; 64];
            errno::set_errno(0);
            assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 64), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// READ_CLEAR without CAP_SYSLOG → EPERM.
        #[test]
        fn test_klogctl_phase172_read_clear_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            let mut buf = [0u8; 64];
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_READ_CLEAR, buf.as_mut_ptr(), 64),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// CLEAR without CAP_SYSLOG → EPERM.
        #[test]
        fn test_klogctl_phase172_clear_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CLEAR, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// CONSOLE_OFF without CAP_SYSLOG → EPERM.
        #[test]
        fn test_klogctl_phase172_console_off_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CONSOLE_OFF, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// CONSOLE_ON without CAP_SYSLOG → EPERM.
        #[test]
        fn test_klogctl_phase172_console_on_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CONSOLE_ON, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// CONSOLE_LEVEL with valid level but no CAP_SYSLOG → EPERM
        /// (the EINVAL guard for bad level passes; cap probe fires).
        #[test]
        fn test_klogctl_phase172_console_level_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), 4),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Allowed-without-cap (must still reach ENOSYS, not EPERM) ----

        /// CLOSE is a no-op on Linux — no cap required.  Must still
        /// reach ENOSYS (our stub doesn't model the ring buffer).
        #[test]
        fn test_klogctl_phase172_close_no_cap_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CLOSE, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// OPEN is a no-op on Linux — no cap required.
        #[test]
        fn test_klogctl_phase172_open_no_cap_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_OPEN, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// SIZE_BUFFER returns the buffer size on Linux — no cap
        /// required.  Our stub still reaches ENOSYS.
        #[test]
        fn test_klogctl_phase172_size_buffer_no_cap_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_SIZE_BUFFER, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// READ_ALL is the unprivileged dmesg path under
        /// `dmesg_restrict=0` — no cap required.
        #[test]
        fn test_klogctl_phase172_read_all_no_cap_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            let mut buf = [0u8; 64];
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_READ_ALL, buf.as_mut_ptr(), 64),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// SIZE_UNREAD is read-only — no cap required.
        #[test]
        fn test_klogctl_phase172_size_unread_no_cap_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_SIZE_UNREAD, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Ordering matrix (EINVAL beats EPERM) ------------------------

        /// Negative cmd returns EINVAL even with no cap — argument-
        /// domain check runs first.
        #[test]
        fn test_klogctl_phase172_bad_cmd_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(klogctl(-1, core::ptr::null_mut(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// READ with NULL buf returns EINVAL even with no cap — the
        /// buf/len fold-check runs before the cap probe.
        #[test]
        fn test_klogctl_phase172_read_null_buf_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(klogctl(SYSLOG_ACTION_READ, core::ptr::null_mut(), 64), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// CONSOLE_LEVEL with bad level returns EINVAL even with no
        /// cap — the level-range check runs before the cap probe.
        #[test]
        fn test_klogctl_phase172_bad_console_level_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// READ with len==0 short-circuits to success (returns 0)
        /// even with no cap — this matches Linux's `if (!len) return 0;`
        /// fast-path that fires before the permission check would
        /// otherwise apply.  (Linux's `do_syslog` permission check
        /// runs before, but our stub mirrors the observable
        /// fast-path-zero behaviour.)
        #[test]
        fn test_klogctl_phase172_read_zero_len_no_cap_returns_zero() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            let mut buf = [0u8; 16];
            errno::set_errno(0);
            assert_eq!(klogctl(SYSLOG_ACTION_READ, buf.as_mut_ptr(), 0), 0);
            // errno unchanged on success
            assert_eq!(errno::get_errno(), 0);
        }

        // -- Workflow ----------------------------------------------------

        /// systemd-journal-like probe: tries to call CLEAR (privileged)
        /// after the cap is dropped — gets EPERM, falls back to
        /// READ_ALL (unprivileged) which reaches ENOSYS.
        #[test]
        fn test_klogctl_phase172_workflow_drop_then_fallback() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            // First: try CLEAR (privileged action) — EPERM.
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CLEAR, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Then fall back to unprivileged READ_ALL — ENOSYS.
            let mut buf = [0u8; 32];
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_READ_ALL, buf.as_mut_ptr(), 32),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Recovery ----------------------------------------------------

        /// After EPERM from a cap-required action, restoring the cap
        /// allows the privileged path to reach ENOSYS.
        #[test]
        fn test_klogctl_phase172_recovery_after_eperm() {
            let _outer = CapGuard::snapshot();
            // Drop the cap and observe EPERM.
            drop_cap_syslog();
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CLEAR, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Restore via the outer guard's stored state by dropping a
            // nested guard — simplest: just capset the default high
            // bit back.  Use the outer guard's snapshot path: a fresh
            // inner CapGuard snapshots the (cap-dropped) state, but to
            // restore we manually set the CAP_SYSLOG bit.
            use crate::sys_capability::CAP_SYSLOG;
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYSLOG < 32 {
                (lo | (1u32 << CAP_SYSLOG), hi)
            } else {
                (lo, hi | (1u32 << (CAP_SYSLOG - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0);
            assert!(crate::sys_capability::has_capability(CAP_SYSLOG));
            // Now CLEAR reaches ENOSYS.
            errno::set_errno(0);
            assert_eq!(
                klogctl(SYSLOG_ACTION_CLEAR, core::ptr::null_mut(), 0),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Sentinel: cap-held privileged path still works --------------

        /// With CAP_SYSLOG held (default state), every cap-required
        /// action reaches ENOSYS — verifies the gate doesn't fire
        /// when the cap is present.
        #[test]
        fn test_klogctl_phase172_sentinel_cap_held_reaches_enosys() {
            let _g = CapGuard::snapshot();
            // Don't drop the cap — default state holds CAP_SYSLOG.
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYSLOG,
            ));
            let mut buf = [0u8; 32];
            for cmd in [
                SYSLOG_ACTION_READ,
                SYSLOG_ACTION_READ_CLEAR,
                SYSLOG_ACTION_CLEAR,
                SYSLOG_ACTION_CONSOLE_OFF,
                SYSLOG_ACTION_CONSOLE_ON,
            ] {
                errno::set_errno(0);
                let (p, l) = if matches!(
                    cmd,
                    SYSLOG_ACTION_READ | SYSLOG_ACTION_READ_CLEAR,
                ) {
                    (buf.as_mut_ptr(), 32i32)
                } else {
                    (core::ptr::null_mut(), 0i32)
                };
                assert_eq!(klogctl(cmd, p, l), -1, "cmd={cmd}");
                assert_eq!(
                    errno::get_errno(),
                    errno::ENOSYS,
                    "cmd={cmd} should reach ENOSYS with cap held",
                );
            }
        }

        // -- Cross-check: dropping CAP_SYSLOG leaves other caps alone ----

        /// Dropping CAP_SYSLOG must not disturb CAP_SYS_NICE,
        /// CAP_IPC_LOCK, CAP_SYS_ADMIN or any unrelated cap — verifies
        /// the bit-mask is precise.
        #[test]
        fn test_klogctl_phase172_drop_syslog_isolates_other_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_syslog();
            // CAP_SYSLOG is bit 2 of high (cap 34); check other caps
            // in both low and high words remain set.
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_IPC_LOCK,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_CHROOT,
            ));
            // And dropping doesn't accidentally re-enable CAP_SYSLOG.
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYSLOG,
            ));
        }
    }
}
