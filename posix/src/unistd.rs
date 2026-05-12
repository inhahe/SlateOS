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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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

// isatty() is defined in ioctl.rs — it checks the fd table's HandleKind
// rather than hardcoding fd numbers, so it works for any Console fd.

/// Get the real user ID of the calling process.
///
/// Returns 0 (root) since we don't have multi-user support in
/// userspace yet.
#[unsafe(no_mangle)]
pub extern "C" fn getuid() -> UidT {
    0
}

/// Get the effective user ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn geteuid() -> UidT {
    0
}

/// Get the real group ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn getgid() -> GidT {
    0
}

/// Get the effective group ID of the calling process.
#[unsafe(no_mangle)]
pub extern "C" fn getegid() -> GidT {
    0
}

/// Set the user ID of the calling process.
///
/// Stub: succeeds silently (single-user OS, always root).
#[unsafe(no_mangle)]
pub extern "C" fn setuid(_uid: UidT) -> i32 {
    0
}

/// Set the effective user ID of the calling process.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn seteuid(_uid: UidT) -> i32 {
    0
}

/// Set the group ID of the calling process.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn setgid(_gid: GidT) -> i32 {
    0
}

/// Set the effective group ID of the calling process.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn setegid(_gid: GidT) -> i32 {
    0
}

/// Set the real and effective user IDs.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn setreuid(_ruid: UidT, _euid: UidT) -> i32 {
    0
}

/// Set the real and effective group IDs.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn setregid(_rgid: GidT, _egid: GidT) -> i32 {
    0
}

/// Get the supplementary group IDs.
///
/// Returns 0 (no supplementary groups — only group 0).
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn setgroups(_size: usize, _list: *const GidT) -> i32 {
    0
}

/// Get the hostname.
///
/// Returns "localhost" (our OS doesn't have a hostname yet).
#[unsafe(no_mangle)]
pub extern "C" fn gethostname(name: *mut u8, len: usize) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let hostname = b"localhost";
    let needed = hostname.len().wrapping_add(1); // +null
    if len < needed {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }

    let mut idx: usize = 0;
    while idx < hostname.len() {
        if let Some(&byte) = hostname.get(idx) {
            unsafe { *name.add(idx) = byte; }
        }
        idx = idx.wrapping_add(1);
    }
    unsafe { *name.add(idx) = 0; }
    0
}

/// Set an alarm timer.
///
/// Stub: returns 0 (no alarm support — signals not implemented).
#[unsafe(no_mangle)]
pub extern "C" fn alarm(_seconds: u32) -> u32 {
    0
}

/// Suspend until a signal is delivered.
///
/// Stub: sleeps for 1 second then returns -1/EINTR (no signals).
#[unsafe(no_mangle)]
pub extern "C" fn pause() -> i32 {
    let _ = syscall1(SYS_SLEEP, 1_000_000_000_u64);
    errno::set_errno(errno::EINTR);
    -1
}

/// Get configurable system variables.
///
/// Returns the value of the named system variable, or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn sysconf(name: i32) -> i64 {
    match name {
        _SC_PAGESIZE => 16384, // Our OS uses 16 KiB pages.
        _SC_NPROCESSORS_CONF | _SC_NPROCESSORS_ONLN => {
            // TODO: Query actual CPU count from kernel.
            1
        }
        _SC_OPEN_MAX => 256,
        _SC_CLK_TCK => 100,            // 100 Hz timer tick (Linux default).
        _SC_ARG_MAX => 131072,          // 128 KiB argument limit.
        _SC_CHILD_MAX => 1024,          // Max child processes.
        _SC_NGROUPS_MAX => 32,          // Max supplementary groups.
        _SC_VERSION => 200809,          // POSIX.1-2008.
        _SC_HOST_NAME_MAX => 64,        // Max hostname length.
        _SC_LOGIN_NAME_MAX => 256,      // Max login name.
        _SC_LINE_MAX => 2048,           // Max line length.
        _SC_THREADS => 200809,          // POSIX threads supported (version).
        _SC_THREAD_STACK_MIN => 65536,  // 64 KiB minimum thread stack.
        _SC_PHYS_PAGES => 8192,         // ~128 MiB at 16 KiB pages (TODO: query kernel).
        _SC_AVPHYS_PAGES => 4096,       // ~64 MiB available (TODO: query kernel).
        _SC_IOV_MAX => 1024,            // Max iovec entries.
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
#[unsafe(no_mangle)]
pub extern "C" fn getpagesize() -> i32 {
    16384
}

// ---------------------------------------------------------------------------
// pathconf / fpathconf / confstr
// ---------------------------------------------------------------------------

/// POSIX _PC_* constants for pathconf.
pub const PC_LINK_MAX: i32 = 0;
pub const PC_MAX_CANON: i32 = 1;
pub const PC_MAX_INPUT: i32 = 2;
pub const PC_NAME_MAX: i32 = 3;
pub const PC_PATH_MAX: i32 = 4;
pub const PC_PIPE_BUF: i32 = 5;

/// Get configurable pathname variables.
///
/// Returns the value of the named limit for `path`, or -1 if the
/// limit is indeterminate or the name is invalid.
#[unsafe(no_mangle)]
pub extern "C" fn pathconf(_path: *const u8, name: i32) -> i64 {
    // Return the same values regardless of path — we don't have
    // per-filesystem limits yet.
    match name {
        PC_LINK_MAX => 127,                                 // Max hard links.
        PC_MAX_CANON | PC_MAX_INPUT | PC_NAME_MAX => 255,   // Terminal/filename limits.
        PC_PATH_MAX => PATH_MAX as i64,
        PC_PIPE_BUF => 4096,                                // Atomic pipe write size.
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Get configurable pathname variables for an open file.
///
/// Same as pathconf but takes a file descriptor.
#[unsafe(no_mangle)]
pub extern "C" fn fpathconf(_fd: i32, name: i32) -> i64 {
    pathconf(core::ptr::null(), name)
}

/// _CS_* constants for confstr.
pub const CS_PATH: i32 = 0;

/// Get configuration-defined string values.
///
/// If `buf` is non-null and `len` > 0, copies the string into `buf`
/// (null-terminated).  Returns the total length needed (including
/// null), or 0 on error.
#[unsafe(no_mangle)]
pub extern "C" fn confstr(name: i32, buf: *mut u8, len: usize) -> usize {
    let value: &[u8] = if name == CS_PATH {
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
#[unsafe(no_mangle)]
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
        errno::set_errno(errno::ENAMETOOLONG);
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
#[unsafe(no_mangle)]
pub extern "C" fn sync() {
    // No-op: filesystem writes are synchronous.
}

/// Set the system hostname.
///
/// Stub: returns -1 with `EPERM`.  Hostname is currently hardcoded
/// to "localhost" in `gethostname()`.
#[unsafe(no_mangle)]
pub extern "C" fn sethostname(_name: *const u8, _len: usize) -> i32 {
    errno::set_errno(errno::EPERM);
    -1
}

/// Change the root directory.
///
/// Stub: returns -1 with `ENOSYS`.  Filesystem namespaces are not
/// yet implemented.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn getrandom(buf: *mut u8, buflen: usize, _flags: u32) -> isize {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    fill_random(buf, buflen);
    buflen as isize
}

/// Fill a buffer with random bytes (simplified API).
///
/// Like `getrandom` but with no flags and returns 0/errno.
/// Maximum 256 bytes per call (POSIX requirement).
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn fdatasync(fd: Fd) -> i32 {
    // Our fsync already just returns 0 (filesystem writes are sync).
    crate::file::fsync(fd)
}

/// Synchronize all data for the filesystem containing `fd`.
///
/// Stub: no-op (same as `sync` — our writes are synchronous).
#[unsafe(no_mangle)]
pub extern "C" fn syncfs(_fd: Fd) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// abort
// ---------------------------------------------------------------------------

/// Write a message to standard error and abort.
///
/// Not exactly POSIX, but commonly needed by C runtime init code.
#[unsafe(no_mangle)]
pub extern "C" fn abort() -> ! {
    // Write "Aborted\n" to stderr (console).
    let msg = b"Aborted\n";
    let _ = syscall2(SYS_CONSOLE_WRITE, msg.as_ptr() as u64, msg.len() as u64);
    #[allow(clippy::used_underscore_items)] // _exit is the POSIX name.
    crate::process::_exit(134); // 128 + SIGABRT(6)
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
}
