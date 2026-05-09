//! POSIX process spawning functions.
//!
//! Implements `posix_spawn`, `posix_spawnp`, `execve`, `execvp`, and
//! `execv`.
//!
//! ## How It Works
//!
//! Our kernel's `SYS_PROCESS_SPAWN` and `SYS_PROCESS_EXEC` take raw
//! ELF data in memory, not file paths.  This module bridges the gap:
//!
//! 1. Stat the file to determine its size
//! 2. Allocate a buffer via mmap
//! 3. Read the ELF binary from the filesystem via `SYS_FS_READ_FILE`
//! 4. Pass the raw bytes to `SYS_PROCESS_SPAWN` or `SYS_PROCESS_EXEC`
//! 5. Free the buffer via munmap
//!
//! ## PATH Search
//!
//! `posix_spawnp` and `execvp` support PATH-based executable lookup.
//! If the filename contains a `/`, it is used directly.  Otherwise,
//! each directory in the `PATH` environment variable (or the default
//! `/bin:/usr/bin`) is tried with the filename appended.  The first
//! path that exists (per `SYS_FS_STAT`) is used.
//!
//! ## Limitations
//!
//! - `posix_spawn` file_actions and attrp are ignored (not yet implemented).
//! - `argv` and `envp` are not yet passed to the child process.

use crate::errno;
use crate::mman;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// posix_spawn
// ---------------------------------------------------------------------------

/// Spawn a new process from a file path.
///
/// Reads the ELF binary at `path` and creates a new process.
/// On success, stores the child PID in `*pid` (if non-null).
///
/// # Parameters
///
/// - `pid`: Output parameter for child PID (may be null).
/// - `path`: Path to the ELF binary (null-terminated C string).
/// - `file_actions`: Ignored (not yet implemented).
/// - `attrp`: Ignored (not yet implemented).
/// - `argv`: Ignored (not yet passed to child).
/// - `envp`: Ignored (not yet passed to child).
///
/// Returns 0 on success, or an error number (NOT -1) on failure.
/// This matches the POSIX spec: `posix_spawn` returns the error
/// directly, not via errno.
#[unsafe(no_mangle)]
pub extern "C" fn posix_spawn(
    pid: *mut PidT,
    path: *const u8,
    _file_actions: *const core::ffi::c_void,
    _attrp: *const core::ffi::c_void,
    _argv: *const *const u8,
    _envp: *const *const u8,
) -> i32 {
    if path.is_null() {
        return errno::EINVAL;
    }

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        return errno::ENAMETOOLONG;
    };

    // Load the ELF binary using the resolved absolute path.
    let (buf_ptr, elf_len) = match load_elf(resolved.as_ptr(), resolved_len) {
        Ok(result) => result,
        Err(err) => return err,
    };

    // Spawn the process with the ELF data.
    let ret = syscall4(
        SYS_PROCESS_SPAWN,
        buf_ptr as u64,
        elf_len as u64,
        resolved.as_ptr() as u64,  // Use resolved path as the process name.
        resolved_len as u64,
    );

    // Free the ELF buffer.
    let _ = mman::munmap(buf_ptr.cast::<core::ffi::c_void>(), elf_len);

    if ret < 0 {
        return native_to_posix_err(ret);
    }

    // Store child PID if requested.
    if !pid.is_null() {
        unsafe { *pid = ret as PidT; }
    }

    0
}

/// Spawn a new process, searching the PATH for the executable.
///
/// Like `posix_spawn` but `file` is searched for in the directories
/// listed in the `PATH` environment variable.  If `file` contains a
/// `/`, it is used directly without PATH search.
///
/// Returns 0 on success, or an error number on failure.
#[unsafe(no_mangle)]
pub extern "C" fn posix_spawnp(
    pid: *mut PidT,
    file: *const u8,
    file_actions: *const core::ffi::c_void,
    attrp: *const core::ffi::c_void,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    if file.is_null() {
        return errno::EINVAL;
    }

    // If `file` contains a '/', use it directly (no PATH search).
    let file_len = unsafe { crate::file::c_strlen_pub(file) };
    if contains_slash(file, file_len) {
        return posix_spawn(pid, file, file_actions, attrp, argv, envp);
    }

    // Search PATH for the executable.
    let mut found = [0u8; crate::unistd::PATH_MAX];
    if !search_path(file, file_len, &mut found) {
        return errno::ENOENT;
    }

    posix_spawn(pid, found.as_ptr(), file_actions, attrp, argv, envp)
}

// ---------------------------------------------------------------------------
// execve (proper implementation)
// ---------------------------------------------------------------------------

/// Replace the current process image with a new program.
///
/// Reads the ELF binary at `path` and calls `SYS_PROCESS_EXEC` to
/// replace the current process.  On success, this function does not
/// return.  On failure, returns -1 with errno set.
#[unsafe(no_mangle)]
pub extern "C" fn execve(
    path: *const u8,
    _argv: *const *const u8,
    _envp: *const *const u8,
) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    };

    // Load the ELF binary using the resolved absolute path.
    let (buf_ptr, elf_len) = match load_elf(resolved.as_ptr(), resolved_len) {
        Ok(result) => result,
        Err(err) => {
            errno::set_errno(err);
            return -1;
        }
    };

    // Replace the current process image.
    let ret = syscall2(
        SYS_PROCESS_EXEC,
        buf_ptr as u64,
        elf_len as u64,
    );

    // If we get here, exec failed.  Free the buffer.
    let _ = mman::munmap(buf_ptr.cast::<core::ffi::c_void>(), elf_len);
    let _ = errno::translate(ret);
    -1
}

// ---------------------------------------------------------------------------
// execvp
// ---------------------------------------------------------------------------

/// Replace the current process image, searching PATH for the executable.
///
/// Like `execve` but `file` is searched for in the directories listed
/// in the `PATH` environment variable.  If `file` contains a `/`, it
/// is used directly without PATH search.
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[unsafe(no_mangle)]
pub extern "C" fn execvp(
    file: *const u8,
    argv: *const *const u8,
) -> i32 {
    if file.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let file_len = unsafe { crate::file::c_strlen_pub(file) };

    // If `file` contains a '/', use it directly.
    if contains_slash(file, file_len) {
        // Pass null envp — execve ignores it currently anyway.
        return execve(file, argv, core::ptr::null());
    }

    // Search PATH for the executable.
    let mut found = [0u8; crate::unistd::PATH_MAX];
    if !search_path(file, file_len, &mut found) {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    execve(found.as_ptr(), argv, core::ptr::null())
}

// ---------------------------------------------------------------------------
// execv
// ---------------------------------------------------------------------------

/// Replace the current process image with a new program.
///
/// Like `execve` but inherits the current environment (the `envp`
/// parameter is omitted).
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[unsafe(no_mangle)]
pub extern "C" fn execv(
    path: *const u8,
    argv: *const *const u8,
) -> i32 {
    execve(path, argv, core::ptr::null())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load an ELF binary from the filesystem into an mmap'd buffer.
///
/// Returns `(buffer_ptr, file_size)` on success, or a POSIX error
/// number on failure.
fn load_elf(path: *const u8, path_len: usize) -> Result<(*mut u8, usize), i32> {
    // Stat the file to get its size.
    let mut stat_buf = crate::stat::Stat::zeroed();
    let stat_ret = syscall3(
        SYS_FS_STAT,
        path as u64,
        path_len as u64,
        (&raw mut stat_buf) as u64,
    );

    if stat_ret < 0 {
        return Err(native_to_posix_err(stat_ret));
    }

    let file_size = stat_buf.st_size as usize;
    if file_size == 0 {
        return Err(errno::ENOEXEC);
    }

    // Allocate a buffer via mmap.
    let buf = mman::mmap(
        core::ptr::null_mut(),
        file_size,
        mman::PROT_READ | mman::PROT_WRITE,
        mman::MAP_PRIVATE | mman::MAP_ANONYMOUS,
        -1,
        0,
    );

    if buf == mman::MAP_FAILED {
        return Err(errno::ENOMEM);
    }

    let buf_ptr = buf.cast::<u8>();

    // Read the ELF binary into the buffer.
    let read_ret = syscall4(
        SYS_FS_READ_FILE,
        path as u64,
        path_len as u64,
        buf_ptr as u64,
        file_size as u64,
    );

    if read_ret < 0 {
        let _ = mman::munmap(buf, file_size);
        return Err(native_to_posix_err(read_ret));
    }

    let bytes_read = read_ret as usize;
    if bytes_read == 0 {
        let _ = mman::munmap(buf, file_size);
        return Err(errno::ENOEXEC);
    }

    Ok((buf_ptr, bytes_read))
}

/// Convert a native kernel error code to a POSIX errno value.
///
/// Unlike `errno::translate`, this doesn't set the global errno —
/// it just returns the POSIX error number.  Used by `posix_spawn`
/// which returns errors directly instead of via errno.
#[must_use]
fn native_to_posix_err(ret: i64) -> i32 {
    // Set errno via translate, then read it back.
    // This is slightly wasteful but keeps the mapping in one place.
    let _ = errno::translate(ret);
    errno::get_errno()
}

/// Check whether a byte string contains a `/` character.
///
/// Used by `posix_spawnp` and `execvp` to decide whether to do a
/// PATH search (no slash) or use the path directly (has slash).
fn contains_slash(s: *const u8, len: usize) -> bool {
    let mut i: usize = 0;
    while i < len {
        // SAFETY: Caller guarantees `s` is readable for `len` bytes.
        if unsafe { *s.add(i) } == b'/' {
            return true;
        }
        i = i.wrapping_add(1);
    }
    false
}

/// Default PATH used when the PATH environment variable is not set.
const DEFAULT_PATH: &[u8] = b"/bin:/usr/bin";

/// Search the PATH environment variable for an executable file.
///
/// Tries each directory in PATH with `file` appended.  Returns `true`
/// if found, writing the full null-terminated path into `out`.
///
/// The search checks existence via `SYS_FS_STAT` — it does not check
/// execute permission (our OS doesn't have a permission system yet).
fn search_path(
    file: *const u8,
    file_len: usize,
    out: &mut [u8; crate::unistd::PATH_MAX],
) -> bool {
    // Get the PATH environment variable.
    // SAFETY: "PATH\0" is a valid C string.
    let path_env = unsafe { crate::environ::getenv(c"PATH".as_ptr().cast::<u8>()) };

    // Determine the PATH string and its length.
    let (path_ptr, path_total_len) = if path_env.is_null() {
        (DEFAULT_PATH.as_ptr(), DEFAULT_PATH.len())
    } else {
        let len = unsafe { crate::string::strlen(path_env) };
        (path_env, len)
    };

    // Iterate over ':'-delimited directory components.
    let mut start: usize = 0;
    while start <= path_total_len {
        // Find the end of this component (next ':' or end of string).
        let mut end = start;
        while end < path_total_len {
            // SAFETY: `end < path_total_len` guarantees readable.
            if unsafe { *path_ptr.add(end) } == b':' {
                break;
            }
            end = end.wrapping_add(1);
        }

        let dir_len = end.wrapping_sub(start);

        // Skip empty components (e.g., leading/trailing/double ':').
        if dir_len > 0 {
            // Build "dir/file" in `out`.  Need: dir_len + 1 (slash) + file_len < PATH_MAX.
            let total = dir_len.wrapping_add(1).wrapping_add(file_len);
            if total < crate::unistd::PATH_MAX {
                // Copy directory.
                let mut pos: usize = 0;
                let mut j: usize = 0;
                while j < dir_len {
                    if let Some(slot) = out.get_mut(pos) {
                        // SAFETY: `start + j < path_total_len` guarantees readable.
                        *slot = unsafe { *path_ptr.add(start.wrapping_add(j)) };
                    }
                    pos = pos.wrapping_add(1);
                    j = j.wrapping_add(1);
                }

                // Add separator '/'.
                if let Some(slot) = out.get_mut(pos) {
                    *slot = b'/';
                }
                pos = pos.wrapping_add(1);

                // Copy filename.
                let mut k: usize = 0;
                while k < file_len {
                    if let Some(slot) = out.get_mut(pos) {
                        // SAFETY: `k < file_len` and caller guarantees
                        // `file` is readable for `file_len` bytes.
                        *slot = unsafe { *file.add(k) };
                    }
                    pos = pos.wrapping_add(1);
                    k = k.wrapping_add(1);
                }

                // Null-terminate.
                if let Some(slot) = out.get_mut(pos) {
                    *slot = 0;
                }

                // Check if this path exists via SYS_FS_STAT.
                if file_exists(out.as_ptr(), pos) {
                    return true;
                }
            }
        }

        // Advance past the ':' (or past end to terminate the loop).
        start = end.wrapping_add(1);
    }

    false
}

/// Check whether a file exists at the given path.
///
/// Uses `SYS_FS_STAT` to test existence.  Does not check file type
/// or permissions — just whether stat succeeds.
fn file_exists(path: *const u8, path_len: usize) -> bool {
    let mut stat_buf = crate::stat::Stat::zeroed();
    let ret = syscall3(
        SYS_FS_STAT,
        path as u64,
        path_len as u64,
        (&raw mut stat_buf) as u64,
    );
    ret >= 0
}
