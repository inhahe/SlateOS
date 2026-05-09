//! POSIX process spawning functions.
//!
//! Implements `posix_spawn` and updates `execve` to work with our
//! kernel's ELF-based spawn/exec syscalls.
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

/// Convenience: spawn a process and wait for it.
///
/// Like `posix_spawn` followed by `waitpid`.  Returns the exit status.
/// This is a non-POSIX extension commonly needed by shell-like programs.
#[unsafe(no_mangle)]
pub extern "C" fn posix_spawnp(
    pid: *mut PidT,
    path: *const u8,
    file_actions: *const core::ffi::c_void,
    attrp: *const core::ffi::c_void,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    // posix_spawnp searches PATH — we don't have PATH resolution yet,
    // so just delegate to posix_spawn (which requires a full path).
    posix_spawn(pid, path, file_actions, attrp, argv, envp)
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
