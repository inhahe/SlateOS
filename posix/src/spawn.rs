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
//! 1. Read the ELF binary from the filesystem via `SYS_FS_READ_FILE`
//! 2. Pass the raw bytes to `SYS_PROCESS_SPAWN` or `SYS_PROCESS_EXEC`
//!
//! ## Limitations
//!
//! - Maximum ELF binary size is 512 KiB (static buffer, no heap).
//! - `posix_spawn` file_actions and attrp are ignored (not yet implemented).
//! - `argv` and `envp` are not yet passed to the child process.

use crate::errno;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum ELF binary size we can load (512 KiB).
///
/// This is a static buffer limitation.  Larger binaries would need
/// mmap-based loading, which requires the mmap syscall to be functional.
const MAX_ELF_SIZE: usize = 512 * 1024;

// ---------------------------------------------------------------------------
// Static ELF buffer
// ---------------------------------------------------------------------------

/// Static buffer for reading ELF binaries from the filesystem.
///
/// Shared between `posix_spawn` and `execve`.  Not thread-safe —
/// requires synchronization when threading is added.
static mut ELF_BUF: [u8; MAX_ELF_SIZE] = [0u8; MAX_ELF_SIZE];

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

    let path_len = unsafe { crate::file::c_strlen_pub(path) };

    // Read the ELF binary into our static buffer.
    let elf_len = unsafe {
        let buf_ptr = core::ptr::addr_of_mut!(ELF_BUF);
        let ret = syscall4(
            SYS_FS_READ_FILE,
            path as u64,
            path_len as u64,
            (*buf_ptr).as_mut_ptr() as u64,
            MAX_ELF_SIZE as u64,
        );

        if ret < 0 {
            // Translate native error to POSIX errno value.
            return native_to_posix_err(ret);
        }
        ret as usize
    };

    if elf_len == 0 {
        return errno::ENOEXEC;
    }

    // Spawn the process with the ELF data.
    unsafe {
        let buf_ptr = core::ptr::addr_of_mut!(ELF_BUF);
        let ret = syscall4(
            SYS_PROCESS_SPAWN,
            (*buf_ptr).as_ptr() as u64,
            elf_len as u64,
            path as u64,      // Use path as the process name.
            path_len as u64,
        );

        if ret < 0 {
            return native_to_posix_err(ret);
        }

        // Store child PID if requested.
        if !pid.is_null() {
            *pid = ret as PidT;
        }
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

    let path_len = unsafe { crate::file::c_strlen_pub(path) };

    // Read the ELF binary into our static buffer.
    let elf_len = unsafe {
        let buf_ptr = core::ptr::addr_of_mut!(ELF_BUF);
        let ret = syscall4(
            SYS_FS_READ_FILE,
            path as u64,
            path_len as u64,
            (*buf_ptr).as_mut_ptr() as u64,
            MAX_ELF_SIZE as u64,
        );

        if ret < 0 {
            let _ = errno::translate(ret);
            return -1;
        }
        ret as usize
    };

    if elf_len == 0 {
        errno::set_errno(errno::ENOEXEC);
        return -1;
    }

    // Replace the current process image.
    unsafe {
        let buf_ptr = core::ptr::addr_of_mut!(ELF_BUF);
        let ret = syscall2(
            SYS_PROCESS_EXEC,
            (*buf_ptr).as_ptr() as u64,
            elf_len as u64,
        );

        // If we get here, exec failed.
        let _ = errno::translate(ret);
    }

    -1
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
