//! POSIX process spawning functions.
//!
//! Implements `posix_spawn`, `posix_spawnp`, `execve`, `execvp`, and
//! `execv`.
//!
//! ## How It Works
//!
//! Our kernel's `SYS_PROCESS_SPAWN_EX` and `SYS_PROCESS_EXEC` take raw
//! ELF data in memory, not file paths.  This module bridges the gap:
//!
//! 1. Stat the file to determine its size
//! 2. Allocate a buffer via mmap
//! 3. Read the ELF binary from the filesystem via `SYS_FS_READ_FILE`
//! 4. Pass the raw bytes to `SYS_PROCESS_SPAWN_EX` (with argv/envp)
//!    or `SYS_PROCESS_EXEC`
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
//! ## Argument and Environment Passing
//!
//! `posix_spawn` packs `argv` and `envp` C string arrays into contiguous
//! null-terminated buffers and passes them to the kernel via the
//! `SpawnExArgs` struct.  The child retrieves them during startup via
//! `SYS_PROCESS_GET_ARGS` (handled in `crt.rs`).
//!
//! `execve` passes argv/envp via `SYS_PROCESS_EXEC` args 2–5.
//!
//! ## Limitations
//!
//! - `posix_spawn` file_actions are recorded but not yet applied via
//!   the fd_map mechanism.  The kernel supports fd inheritance via
//!   `SYS_PROCESS_SPAWN_EX` fd_map, but the child-side fd retrieval
//!   (`SYS_PROCESS_GET_INITIAL_FDS`) is not wired into child startup
//!   yet.  File_actions will be effective once the child's `_start`
//!   calls `SYS_PROCESS_GET_INITIAL_FDS` and reinitializes its fd table.
//! - `posix_spawnattr` flags are stored but only `POSIX_SPAWN_SETPGROUP`
//!   is meaningfully supported (spawn attributes are recorded for
//!   forward compatibility).

use crate::errno;
use crate::mman;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// ABI types (must match kernel's proc/spawn.rs layout)
// ---------------------------------------------------------------------------

/// Extended spawn arguments struct passed to `SYS_PROCESS_SPAWN_EX`.
///
/// A single pointer to this struct is passed in arg0.  All pointer
/// fields must point to valid memory for the duration of the syscall.
/// Layout must match kernel's `SpawnExArgs` exactly (C ABI, all u64).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SpawnExArgs {
    /// Pointer to ELF data in memory.
    pub elf_ptr: u64,
    /// Length of ELF data in bytes.
    pub elf_len: u64,
    /// Pointer to process name string (UTF-8).
    pub name_ptr: u64,
    /// Length of name string in bytes.
    pub name_len: u64,
    /// Pointer to `FdMapEntry` array (0 = no fd inheritance).
    pub fd_map_ptr: u64,
    /// Number of `FdMapEntry` entries.
    pub fd_map_count: u64,
    /// Pointer to packed null-terminated argv string data.
    pub argv_ptr: u64,
    /// Total byte length of the packed argv data.
    pub argv_len: u64,
    /// Number of arguments.
    pub argc: u64,
    /// Pointer to packed null-terminated envp string data.
    pub envp_ptr: u64,
    /// Total byte length of the packed envp data.
    pub envp_len: u64,
    /// Number of environment variables.
    pub envc: u64,
}

/// Header returned by `SYS_PROCESS_GET_ARGS`.
///
/// Prefixed to the output buffer, followed by packed argv strings
/// then packed envp strings.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SpawnArgsHeader {
    /// Number of argv entries.
    pub argc: u32,
    /// Number of envp entries.
    pub envc: u32,
    /// Total bytes of packed argv data (including null terminators).
    pub argv_data_len: u32,
    /// Total bytes of packed envp data (including null terminators).
    pub envp_data_len: u32,
}

// ---------------------------------------------------------------------------
// posix_spawn_file_actions
// ---------------------------------------------------------------------------

/// Maximum number of file actions per spawn.
///
/// Covers typical shell pipeline needs (a few close + dup2 pairs).
const MAX_FILE_ACTIONS: usize = 16;

/// Maximum path length stored inline in an open action.
const ACTION_PATH_MAX: usize = 256;

/// A single file action to execute in the child (POSIX order).
#[derive(Clone, Copy)]
// ALLOW: The large Open variant is intentional — all storage is inline
// (no heap) so that FileAction is Copy and fits in fixed-size arrays
// without dynamic allocation.  The size difference is acceptable here.
#[allow(clippy::large_enum_variant)]
#[allow(dead_code)] // Used when posix_spawn actually applies actions in child.
enum FileAction {
    /// Close a file descriptor.
    Close { fd: Fd },
    /// Duplicate `fd` to `newfd` (like dup2).
    Dup2 { fd: Fd, newfd: Fd },
    /// Open `path` with `oflag`/`mode` and assign to `fd`.
    Open {
        fd: Fd,
        path: [u8; ACTION_PATH_MAX],
        path_len: usize,
        oflag: i32,
        mode: ModeT,
    },
}

/// File actions object for `posix_spawn`.
///
/// Stores up to `MAX_FILE_ACTIONS` actions that should be executed in
/// the child process between fork and exec.  Actions are applied in the
/// order they were added (POSIX requirement).
///
/// This struct is laid out at a fixed size so it can be embedded in
/// C-visible structs without heap allocation.
#[repr(C)]
pub struct PosixSpawnFileActionsT {
    /// Number of actions stored.
    count: usize,
    /// Action storage (inline, no heap).
    actions: [FileActionSlot; MAX_FILE_ACTIONS],
    /// Padding to reach a consistent size for C ABI compatibility.
    _pad: [u8; 8],
}

/// Internal slot — wraps `Option<FileAction>` in a fixed-size repr.
#[derive(Clone, Copy)]
#[repr(C)]
struct FileActionSlot {
    /// 0 = empty, 1 = Close, 2 = Dup2, 3 = Open.
    tag: u8,
    fd: Fd,
    newfd: Fd,
    oflag: i32,
    mode: ModeT,
    path: [u8; ACTION_PATH_MAX],
    path_len: usize,
}

impl FileActionSlot {
    const fn empty() -> Self {
        Self {
            tag: 0,
            fd: 0,
            newfd: 0,
            oflag: 0,
            mode: 0,
            path: [0; ACTION_PATH_MAX],
            path_len: 0,
        }
    }

    #[allow(dead_code)] // Used when posix_spawn actually applies actions in child.
    fn to_action(self) -> Option<FileAction> {
        match self.tag {
            1 => Some(FileAction::Close { fd: self.fd }),
            2 => Some(FileAction::Dup2 { fd: self.fd, newfd: self.newfd }),
            3 => Some(FileAction::Open {
                fd: self.fd,
                path: self.path,
                path_len: self.path_len,
                oflag: self.oflag,
                mode: self.mode,
            }),
            _ => None,
        }
    }
}

/// Initialize a file actions object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_init(
    acts: *mut PosixSpawnFileActionsT,
) -> i32 {
    if acts.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: `acts` is non-null and caller guarantees it points to
    // writable memory of at least `size_of::<PosixSpawnFileActionsT>()`.
    unsafe {
        (*acts).count = 0;
        let mut i: usize = 0;
        while i < MAX_FILE_ACTIONS {
            if let Some(slot) = (*acts).actions.get_mut(i) {
                *slot = FileActionSlot::empty();
            }
            i = i.wrapping_add(1);
        }
    }
    0
}

/// Destroy a file actions object.
///
/// No heap resources to free — just zeroes the count.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_destroy(
    acts: *mut PosixSpawnFileActionsT,
) -> i32 {
    if !acts.is_null() {
        // SAFETY: acts is non-null (checked above).
        unsafe { (*acts).count = 0; }
    }
    0
}

/// Add a close action.
///
/// The fd will be closed in the child before exec.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addclose(
    acts: *mut PosixSpawnFileActionsT,
    fd: Fd,
) -> i32 {
    if acts.is_null() || fd < 0 {
        return errno::EINVAL;
    }
    // SAFETY: acts is non-null (checked above).
    let a = unsafe { &mut *acts };
    if a.count >= MAX_FILE_ACTIONS {
        return errno::ENOMEM;
    }
    if let Some(slot) = a.actions.get_mut(a.count) {
        *slot = FileActionSlot { tag: 1, fd, ..FileActionSlot::empty() };
    }
    a.count = a.count.wrapping_add(1);
    0
}

/// Add a dup2 action.
///
/// In the child, `dup2(fd, newfd)` will be called before exec.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_adddup2(
    acts: *mut PosixSpawnFileActionsT,
    fd: Fd,
    newfd: Fd,
) -> i32 {
    if acts.is_null() || fd < 0 || newfd < 0 {
        return errno::EINVAL;
    }
    // SAFETY: acts is non-null (checked above).
    let a = unsafe { &mut *acts };
    if a.count >= MAX_FILE_ACTIONS {
        return errno::ENOMEM;
    }
    if let Some(slot) = a.actions.get_mut(a.count) {
        *slot = FileActionSlot { tag: 2, fd, newfd, ..FileActionSlot::empty() };
    }
    a.count = a.count.wrapping_add(1);
    0
}

/// Add an open action.
///
/// In the child, the file at `path` will be opened with `oflag`/`mode`
/// and the resulting fd will be dup2'd to `fd`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addopen(
    acts: *mut PosixSpawnFileActionsT,
    fd: Fd,
    path: *const u8,
    oflag: i32,
    mode: ModeT,
) -> i32 {
    if acts.is_null() || path.is_null() || fd < 0 {
        return errno::EINVAL;
    }
    // SAFETY: acts and path are non-null (checked above).
    let a = unsafe { &mut *acts };
    if a.count >= MAX_FILE_ACTIONS {
        return errno::ENOMEM;
    }
    let path_len = unsafe { crate::file::c_strlen_pub(path) };
    if path_len >= ACTION_PATH_MAX {
        return errno::ENAMETOOLONG;
    }
    let mut stored_path = [0u8; ACTION_PATH_MAX];
    // SAFETY: path is readable for path_len bytes per c_strlen_pub contract.
    unsafe {
        core::ptr::copy_nonoverlapping(path, stored_path.as_mut_ptr(), path_len);
    }
    if let Some(slot) = a.actions.get_mut(a.count) {
        *slot = FileActionSlot {
            tag: 3,
            fd,
            oflag,
            mode,
            path: stored_path,
            path_len,
            ..FileActionSlot::empty()
        };
    }
    a.count = a.count.wrapping_add(1);
    0
}

// ---------------------------------------------------------------------------
// posix_spawnattr
// ---------------------------------------------------------------------------

/// Spawn attribute flags.
#[allow(dead_code)] // Forward-compatible flag constants.
const POSIX_SPAWN_RESETIDS: i16 = 0x01;
#[allow(dead_code)]
const POSIX_SPAWN_SETPGROUP: i16 = 0x02;
#[allow(dead_code)]
const POSIX_SPAWN_SETSIGDEF: i16 = 0x04;
#[allow(dead_code)]
const POSIX_SPAWN_SETSIGMASK: i16 = 0x08;

/// Spawn attributes object.
///
/// Stores flags and optional process group.  Signal mask and signal
/// defaults are stored for API compatibility but not yet applied
/// (our OS doesn't have POSIX signals).
#[repr(C)]
pub struct PosixSpawnattrT {
    /// Attribute flags (bitwise OR of POSIX_SPAWN_* constants).
    flags: i16,
    /// Process group ID (used if POSIX_SPAWN_SETPGROUP is set).
    pgroup: PidT,
    /// Padding for ABI compatibility.
    _pad: [u8; 328],
}

/// Initialize a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_init(
    attr: *mut PosixSpawnattrT,
) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: attr is non-null (checked above).
    unsafe {
        (*attr).flags = 0;
        (*attr).pgroup = 0;
    }
    0
}

/// Destroy a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_destroy(
    _attr: *mut PosixSpawnattrT,
) -> i32 {
    0 // No resources to free.
}

/// Set flags on a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setflags(
    attr: *mut PosixSpawnattrT,
    flags: i16,
) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: attr is non-null (checked above).
    unsafe { (*attr).flags = flags; }
    0
}

/// Get flags from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getflags(
    attr: *const PosixSpawnattrT,
    flags: *mut i16,
) -> i32 {
    if attr.is_null() || flags.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe { *flags = (*attr).flags; }
    0
}

/// Set the process group in a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setpgroup(
    attr: *mut PosixSpawnattrT,
    pgroup: PidT,
) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: attr is non-null (checked above).
    unsafe { (*attr).pgroup = pgroup; }
    0
}

/// Get the process group from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getpgroup(
    attr: *const PosixSpawnattrT,
    pgroup: *mut PidT,
) -> i32 {
    if attr.is_null() || pgroup.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe { *pgroup = (*attr).pgroup; }
    0
}

// ---------------------------------------------------------------------------
// posix_spawn
// ---------------------------------------------------------------------------

/// Spawn a new process from a file path.
///
/// Reads the ELF binary at `path` and creates a new process via
/// `SYS_PROCESS_SPAWN_EX`.  On success, stores the child PID in
/// `*pid` (if non-null).
///
/// # Parameters
///
/// - `pid`: Output parameter for child PID (may be null).
/// - `path`: Path to the ELF binary (null-terminated C string).
/// - `file_actions`: File actions to apply in the child (close, dup2, open).
///   Currently recorded but not yet applied via the kernel's fd_map
///   mechanism — requires child-side `SYS_PROCESS_GET_INITIAL_FDS`
///   retrieval to be wired into the child's startup code.
/// - `attrp`: Spawn attributes (flags, process group).  Recorded but most
///   flags have no effect yet.
/// - `argv`: Null-terminated array of argument strings for the child.
///   Packed and passed to the kernel; the child retrieves them via
///   `SYS_PROCESS_GET_ARGS` during startup.  May be null.
/// - `envp`: Null-terminated array of environment strings for the child.
///   Packed and passed to the kernel.  May be null.
///
/// Returns 0 on success, or an error number (NOT -1) on failure.
/// This matches the POSIX spec: `posix_spawn` returns the error
/// directly, not via errno.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn(
    pid: *mut PidT,
    path: *const u8,
    file_actions: *const PosixSpawnFileActionsT,
    _attrp: *const PosixSpawnattrT,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    // Note: file_actions are properly stored but cannot be applied to
    // the child yet because the child's startup doesn't call
    // SYS_PROCESS_GET_INITIAL_FDS to retrieve inherited fds.  When
    // the child's _start is updated to do this, we'll build an fd_map
    // from file_actions and the parent's fd table.
    let _action_count = if file_actions.is_null() {
        0
    } else {
        // SAFETY: file_actions is non-null (checked above).
        unsafe { (*file_actions).count }
    };
    if path.is_null() {
        return errno::EINVAL;
    }

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        // POSIX: empty path → ENOENT; too-long → ENAMETOOLONG.
        // SAFETY: path is non-null (checked above) and a valid C string.
        return if unsafe { *path } == 0 { errno::ENOENT } else { errno::ENAMETOOLONG };
    };

    // Load the ELF binary using the resolved absolute path.
    let (buf_ptr, alloc_size, data_size) = match load_elf(resolved.as_ptr(), resolved_len) {
        Ok(result) => result,
        Err(err) => return err,
    };

    // Pack argv into a contiguous null-terminated buffer.
    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let argv_packed_len = pack_cstring_array(argv, &mut argv_buf);
    let argc = count_cstring_array(argv);

    // Pack envp into a contiguous null-terminated buffer.
    let mut envp_buf = [0u8; EXEC_PACKED_MAX];
    let envp_packed_len = pack_cstring_array(envp, &mut envp_buf);
    let envc = count_cstring_array(envp);

    // Build the SpawnExArgs struct for SYS_PROCESS_SPAWN_EX.
    let spawn_args = SpawnExArgs {
        elf_ptr: buf_ptr as u64,
        elf_len: data_size as u64,
        name_ptr: resolved.as_ptr() as u64,
        name_len: resolved_len as u64,
        // fd_map not passed yet — requires child-side
        // SYS_PROCESS_GET_INITIAL_FDS retrieval in child startup.
        fd_map_ptr: 0,
        fd_map_count: 0,
        argv_ptr: if argv_packed_len > 0 { argv_buf.as_ptr() as u64 } else { 0 },
        argv_len: argv_packed_len as u64,
        argc: argc as u64,
        envp_ptr: if envp_packed_len > 0 { envp_buf.as_ptr() as u64 } else { 0 },
        envp_len: envp_packed_len as u64,
        envc: envc as u64,
    };

    // Spawn the process with the extended args struct.
    let ret = syscall1(
        SYS_PROCESS_SPAWN_EX,
        (&spawn_args as *const SpawnExArgs) as u64,
    );

    // Free the ELF buffer (must use alloc_size, not data_size, to
    // unmap the entire mmap'd region and avoid memory leaks).
    let _ = mman::munmap(buf_ptr.cast::<core::ffi::c_void>(), alloc_size);

    if ret < 0 {
        return native_to_posix_err(ret);
    }

    // Record the child PID for waitpid(-1, ...) to use later.
    let child_pid = ret as PidT;
    crate::process::record_child_pid(child_pid);

    // Store child PID if requested.
    if !pid.is_null() {
        unsafe { *pid = child_pid; }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnp(
    pid: *mut PidT,
    file: *const u8,
    file_actions: *const PosixSpawnFileActionsT,
    attrp: *const PosixSpawnattrT,
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

/// Maximum size for packed argv/envp buffers during exec.
const EXEC_PACKED_MAX: usize = 128 * 1024;

/// Replace the current process image with a new program.
///
/// Reads the ELF binary at `path` and calls `SYS_PROCESS_EXEC` to
/// replace the current process.  On success, this function does not
/// return.  On failure, returns -1 with errno set.
///
/// `argv` and `envp` are null-terminated arrays of null-terminated C
/// strings.  They are packed into contiguous buffers and passed to the
/// kernel so the new binary can read them via `SYS_PROCESS_GET_ARGS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execve(
    path: *const u8,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        // POSIX: empty path → ENOENT; too-long → ENAMETOOLONG.
        // SAFETY: path is non-null (checked above) and a valid C string.
        errno::set_errno(if unsafe { *path } == 0 { errno::ENOENT } else { errno::ENAMETOOLONG });
        return -1;
    };

    // Load the ELF binary using the resolved absolute path.
    let (buf_ptr, alloc_size, data_size) = match load_elf(resolved.as_ptr(), resolved_len) {
        Ok(result) => result,
        Err(err) => {
            errno::set_errno(err);
            return -1;
        }
    };

    // Pack argv into a contiguous null-terminated buffer.
    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let argv_len = pack_cstring_array(argv, &mut argv_buf);

    // Pack envp into a contiguous null-terminated buffer.
    let mut envp_buf = [0u8; EXEC_PACKED_MAX];
    let envp_len = pack_cstring_array(envp, &mut envp_buf);

    // Replace the current process image with argv/envp.
    let ret = syscall6(
        SYS_PROCESS_EXEC,
        buf_ptr as u64,
        data_size as u64,
        if argv_len > 0 { argv_buf.as_ptr() as u64 } else { 0 },
        argv_len as u64,
        if envp_len > 0 { envp_buf.as_ptr() as u64 } else { 0 },
        envp_len as u64,
    );

    // If we get here, exec failed.  Free the buffer (must use
    // alloc_size to unmap the entire mmap'd region).
    let _ = mman::munmap(buf_ptr.cast::<core::ffi::c_void>(), alloc_size);
    let _ = errno::translate(ret);
    -1
}

/// Pack a null-terminated array of C strings into a contiguous buffer.
///
/// Each string is copied with its null terminator.  Returns the total
/// byte length written.  If `array` is null, returns 0.
fn pack_cstring_array(array: *const *const u8, buf: &mut [u8]) -> usize {
    if array.is_null() {
        return 0;
    }
    let mut pos = 0usize;
    let mut i = 0usize;
    loop {
        // SAFETY: Caller guarantees array is null-terminated.
        let ptr = unsafe { *array.add(i) };
        if ptr.is_null() {
            break;
        }
        let slen = unsafe { crate::file::c_strlen_pub(ptr) };
        // Need slen + 1 bytes (string + null terminator).
        let needed = slen + 1;
        if pos + needed > buf.len() {
            break; // Truncate silently if buffer is full.
        }
        // SAFETY: ptr points to a valid C string of length slen.
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr().add(pos), slen);
        }
        // Explicit null terminator.
        if let Some(b) = buf.get_mut(pos + slen) {
            *b = 0;
        }
        pos += needed;
        i += 1;
    }
    pos
}

/// Count the number of strings in a null-terminated C string array.
///
/// Used to determine `argc`/`envc` for `SpawnExArgs`.
/// Returns 0 if `array` is null.
fn count_cstring_array(array: *const *const u8) -> usize {
    if array.is_null() {
        return 0;
    }
    let mut count = 0usize;
    loop {
        // SAFETY: Caller guarantees array is null-terminated.
        let ptr = unsafe { *array.add(count) };
        if ptr.is_null() {
            break;
        }
        count = count.wrapping_add(1);
    }
    count
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
/// Returns `(buffer_ptr, alloc_size, data_size)` on success, or a POSIX
/// error number on failure.  `alloc_size` is the mmap allocation size
/// (must be used for munmap); `data_size` is the number of bytes
/// actually read (pass to the kernel as the ELF size).
fn load_elf(path: *const u8, path_len: usize) -> Result<(*mut u8, usize, usize), i32> {
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

    Ok((buf_ptr, file_size, bytes_read))
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- FileActionSlot --

    #[test]
    fn test_file_action_slot_empty() {
        let slot = FileActionSlot::empty();
        assert_eq!(slot.tag, 0);
        assert_eq!(slot.fd, 0);
        assert_eq!(slot.newfd, 0);
        assert_eq!(slot.oflag, 0);
        assert_eq!(slot.mode, 0);
        assert_eq!(slot.path_len, 0);
    }

    #[test]
    fn test_file_action_slot_to_action_empty() {
        let slot = FileActionSlot::empty();
        assert!(slot.to_action().is_none());
    }

    #[test]
    fn test_file_action_slot_to_action_close() {
        let slot = FileActionSlot { tag: 1, fd: 5, ..FileActionSlot::empty() };
        let action = slot.to_action();
        assert!(action.is_some());
        match action.unwrap() {
            FileAction::Close { fd } => assert_eq!(fd, 5),
            _ => panic!("expected Close"),
        }
    }

    #[test]
    fn test_file_action_slot_to_action_dup2() {
        let slot = FileActionSlot { tag: 2, fd: 3, newfd: 7, ..FileActionSlot::empty() };
        let action = slot.to_action();
        match action.unwrap() {
            FileAction::Dup2 { fd, newfd } => {
                assert_eq!(fd, 3);
                assert_eq!(newfd, 7);
            }
            _ => panic!("expected Dup2"),
        }
    }

    #[test]
    fn test_file_action_slot_to_action_open() {
        let mut path = [0u8; ACTION_PATH_MAX];
        path[0] = b'/';
        path[1] = b'f';
        path[2] = b'o';
        path[3] = b'o';
        let slot = FileActionSlot {
            tag: 3, fd: 1, oflag: 0x42, mode: 0o644, path, path_len: 4,
            ..FileActionSlot::empty()
        };
        let action = slot.to_action();
        match action.unwrap() {
            FileAction::Open { fd, path: p, path_len, oflag, mode } => {
                assert_eq!(fd, 1);
                assert_eq!(path_len, 4);
                assert_eq!(&p[..4], b"/foo");
                assert_eq!(oflag, 0x42);
                assert_eq!(mode, 0o644);
            }
            _ => panic!("expected Open"),
        }
    }

    #[test]
    fn test_file_action_slot_to_action_invalid_tag() {
        let slot = FileActionSlot { tag: 99, ..FileActionSlot::empty() };
        assert!(slot.to_action().is_none());
    }

    // -- posix_spawn_file_actions_init/destroy --

    #[test]
    fn test_file_actions_init() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        let ret = posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(ret, 0);
        assert_eq!(acts.count, 0);
    }

    #[test]
    fn test_file_actions_init_null() {
        let ret = posix_spawn_file_actions_init(core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_file_actions_destroy() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_destroy(&raw mut acts);
        assert_eq!(ret, 0);
        assert_eq!(acts.count, 0);
    }

    #[test]
    fn test_file_actions_destroy_null() {
        // Destroying null should not crash, returns 0.
        let ret = posix_spawn_file_actions_destroy(core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    // -- posix_spawn_file_actions_addclose --

    #[test]
    fn test_file_actions_addclose() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclose(&raw mut acts, 3);
        assert_eq!(ret, 0);
        assert_eq!(acts.count, 1);
        assert_eq!(acts.actions[0].tag, 1); // Close
        assert_eq!(acts.actions[0].fd, 3);
    }

    #[test]
    fn test_file_actions_addclose_null() {
        let ret = posix_spawn_file_actions_addclose(core::ptr::null_mut(), 3);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_file_actions_addclose_negative_fd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclose(&raw mut acts, -1);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_file_actions_addclose_full() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        // Fill to capacity.
        for i in 0..MAX_FILE_ACTIONS {
            let ret = posix_spawn_file_actions_addclose(&raw mut acts, i as Fd);
            assert_eq!(ret, 0);
        }
        assert_eq!(acts.count, MAX_FILE_ACTIONS);
        // One more should fail.
        let ret = posix_spawn_file_actions_addclose(&raw mut acts, 99);
        assert_eq!(ret, errno::ENOMEM);
    }

    // -- posix_spawn_file_actions_adddup2 --

    #[test]
    fn test_file_actions_adddup2() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_adddup2(&raw mut acts, 3, 1);
        assert_eq!(ret, 0);
        assert_eq!(acts.count, 1);
        assert_eq!(acts.actions[0].tag, 2); // Dup2
        assert_eq!(acts.actions[0].fd, 3);
        assert_eq!(acts.actions[0].newfd, 1);
    }

    #[test]
    fn test_file_actions_adddup2_null() {
        let ret = posix_spawn_file_actions_adddup2(core::ptr::null_mut(), 3, 1);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_file_actions_adddup2_negative_fd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_adddup2(&raw mut acts, -1, 1);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_file_actions_adddup2_negative_newfd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_adddup2(&raw mut acts, 1, -1);
        assert_eq!(ret, errno::EINVAL);
    }

    // -- posix_spawn_file_actions_addopen --

    #[test]
    fn test_file_actions_addopen() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let path = b"/dev/null\0";
        let ret = posix_spawn_file_actions_addopen(
            &raw mut acts, 0, path.as_ptr(), 0, 0o644,
        );
        assert_eq!(ret, 0);
        assert_eq!(acts.count, 1);
        assert_eq!(acts.actions[0].tag, 3); // Open
        assert_eq!(acts.actions[0].fd, 0);
        assert_eq!(acts.actions[0].oflag, 0);
        assert_eq!(acts.actions[0].mode, 0o644);
        assert_eq!(acts.actions[0].path_len, 9); // "/dev/null"
    }

    #[test]
    fn test_file_actions_addopen_null_acts() {
        let path = b"/dev/null\0";
        let ret = posix_spawn_file_actions_addopen(
            core::ptr::null_mut(), 0, path.as_ptr(), 0, 0,
        );
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_file_actions_addopen_null_path() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addopen(
            &raw mut acts, 0, core::ptr::null(), 0, 0,
        );
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_file_actions_addopen_negative_fd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let path = b"/dev/null\0";
        let ret = posix_spawn_file_actions_addopen(
            &raw mut acts, -1, path.as_ptr(), 0, 0,
        );
        assert_eq!(ret, errno::EINVAL);
    }

    // -- posix_spawn_file_actions ordering --

    #[test]
    fn test_file_actions_ordering() {
        // POSIX requires actions to be applied in order.
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);

        posix_spawn_file_actions_addclose(&raw mut acts, 3);
        posix_spawn_file_actions_adddup2(&raw mut acts, 4, 1);
        posix_spawn_file_actions_addclose(&raw mut acts, 5);

        assert_eq!(acts.count, 3);
        // Verify order preserved.
        assert_eq!(acts.actions[0].tag, 1); // Close(3)
        assert_eq!(acts.actions[0].fd, 3);
        assert_eq!(acts.actions[1].tag, 2); // Dup2(4, 1)
        assert_eq!(acts.actions[1].fd, 4);
        assert_eq!(acts.actions[1].newfd, 1);
        assert_eq!(acts.actions[2].tag, 1); // Close(5)
        assert_eq!(acts.actions[2].fd, 5);
    }

    // -- posix_spawnattr_init/destroy --

    #[test]
    fn test_spawnattr_init() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        let ret = posix_spawnattr_init(&raw mut attr);
        assert_eq!(ret, 0);
        assert_eq!(attr.flags, 0);
        assert_eq!(attr.pgroup, 0);
    }

    #[test]
    fn test_spawnattr_init_null() {
        let ret = posix_spawnattr_init(core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_spawnattr_destroy() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_destroy(&raw mut attr);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_spawnattr_destroy_null() {
        let ret = posix_spawnattr_destroy(core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    // -- posix_spawnattr_setflags/getflags --

    #[test]
    fn test_spawnattr_setflags() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_setflags(&raw mut attr, 0x02); // POSIX_SPAWN_SETPGROUP
        assert_eq!(ret, 0);
        assert_eq!(attr.flags, 0x02);
    }

    #[test]
    fn test_spawnattr_setflags_null() {
        let ret = posix_spawnattr_setflags(core::ptr::null_mut(), 0);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_spawnattr_getflags() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        posix_spawnattr_setflags(&raw mut attr, 0x05);
        let mut flags: i16 = 0;
        let ret = posix_spawnattr_getflags(&raw const attr, &raw mut flags);
        assert_eq!(ret, 0);
        assert_eq!(flags, 0x05);
    }

    #[test]
    fn test_spawnattr_getflags_null_attr() {
        let mut flags: i16 = 0;
        let ret = posix_spawnattr_getflags(core::ptr::null(), &raw mut flags);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_spawnattr_getflags_null_out() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_getflags(&raw const attr, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
    }

    // -- posix_spawnattr_setpgroup/getpgroup --

    #[test]
    fn test_spawnattr_setpgroup() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_setpgroup(&raw mut attr, 42);
        assert_eq!(ret, 0);
        assert_eq!(attr.pgroup, 42);
    }

    #[test]
    fn test_spawnattr_setpgroup_null() {
        let ret = posix_spawnattr_setpgroup(core::ptr::null_mut(), 42);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_spawnattr_getpgroup() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        posix_spawnattr_setpgroup(&raw mut attr, 99);
        let mut pg: PidT = 0;
        let ret = posix_spawnattr_getpgroup(&raw const attr, &raw mut pg);
        assert_eq!(ret, 0);
        assert_eq!(pg, 99);
    }

    #[test]
    fn test_spawnattr_getpgroup_null_attr() {
        let mut pg: PidT = 0;
        let ret = posix_spawnattr_getpgroup(core::ptr::null(), &raw mut pg);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_spawnattr_getpgroup_null_out() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_getpgroup(&raw const attr, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
    }

    // -- contains_slash --

    #[test]
    fn test_contains_slash_empty() {
        assert!(!contains_slash(b"\0".as_ptr(), 0));
    }

    #[test]
    fn test_contains_slash_no_slash() {
        assert!(!contains_slash(b"hello\0".as_ptr(), 5));
    }

    #[test]
    fn test_contains_slash_has_slash() {
        assert!(contains_slash(b"/bin/sh\0".as_ptr(), 7));
    }

    #[test]
    fn test_contains_slash_only_slash() {
        assert!(contains_slash(b"/\0".as_ptr(), 1));
    }

    #[test]
    fn test_contains_slash_trailing() {
        assert!(contains_slash(b"foo/\0".as_ptr(), 4));
    }

    // -- Spawn flag constants --

    #[test]
    fn test_spawn_flag_constants() {
        // Verify flag values match POSIX.
        assert_eq!(POSIX_SPAWN_RESETIDS, 0x01);
        assert_eq!(POSIX_SPAWN_SETPGROUP, 0x02);
        assert_eq!(POSIX_SPAWN_SETSIGDEF, 0x04);
        assert_eq!(POSIX_SPAWN_SETSIGMASK, 0x08);
    }

    #[test]
    fn test_spawn_flags_no_overlap() {
        let all = POSIX_SPAWN_RESETIDS | POSIX_SPAWN_SETPGROUP
                 | POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK;
        // Each flag should be a distinct bit.
        assert_eq!(all, 0x0F);
    }

    // -- SpawnExArgs struct layout --

    #[test]
    fn test_spawn_ex_args_size() {
        // SpawnExArgs has 12 u64 fields = 96 bytes.
        assert_eq!(core::mem::size_of::<SpawnExArgs>(), 96);
    }

    #[test]
    fn test_spawn_ex_args_alignment() {
        // Must be u64-aligned for proper ABI.
        assert_eq!(core::mem::align_of::<SpawnExArgs>(), 8);
    }

    #[test]
    fn test_spawn_ex_args_field_layout() {
        // Verify fields are at the expected offsets (all u64, sequential).
        let args = SpawnExArgs {
            elf_ptr: 0x1111_1111_1111_1111,
            elf_len: 0x2222_2222_2222_2222,
            name_ptr: 0x3333_3333_3333_3333,
            name_len: 0x4444_4444_4444_4444,
            fd_map_ptr: 0x5555_5555_5555_5555,
            fd_map_count: 6,
            argv_ptr: 0x7777_7777_7777_7777,
            argv_len: 128,
            argc: 3,
            envp_ptr: 0xAAAA_AAAA_AAAA_AAAA,
            envp_len: 64,
            envc: 2,
        };
        assert_eq!(args.elf_ptr, 0x1111_1111_1111_1111);
        assert_eq!(args.elf_len, 0x2222_2222_2222_2222);
        assert_eq!(args.name_ptr, 0x3333_3333_3333_3333);
        assert_eq!(args.name_len, 0x4444_4444_4444_4444);
        assert_eq!(args.fd_map_ptr, 0x5555_5555_5555_5555);
        assert_eq!(args.fd_map_count, 6);
        assert_eq!(args.argv_ptr, 0x7777_7777_7777_7777);
        assert_eq!(args.argv_len, 128);
        assert_eq!(args.argc, 3);
        assert_eq!(args.envp_ptr, 0xAAAA_AAAA_AAAA_AAAA);
        assert_eq!(args.envp_len, 64);
        assert_eq!(args.envc, 2);
    }

    // -- SpawnArgsHeader struct layout --

    #[test]
    fn test_spawn_args_header_size() {
        // SpawnArgsHeader has 4 u32 fields = 16 bytes.
        assert_eq!(core::mem::size_of::<SpawnArgsHeader>(), 16);
    }

    #[test]
    fn test_spawn_args_header_alignment() {
        assert_eq!(core::mem::align_of::<SpawnArgsHeader>(), 4);
    }

    #[test]
    fn test_spawn_args_header_field_values() {
        let header = SpawnArgsHeader {
            argc: 5,
            envc: 3,
            argv_data_len: 100,
            envp_data_len: 50,
        };
        assert_eq!(header.argc, 5);
        assert_eq!(header.envc, 3);
        assert_eq!(header.argv_data_len, 100);
        assert_eq!(header.envp_data_len, 50);
    }

    // -- count_cstring_array --

    #[test]
    fn test_count_cstring_array_null() {
        assert_eq!(count_cstring_array(core::ptr::null()), 0);
    }

    #[test]
    fn test_count_cstring_array_empty() {
        // A null-terminated array with just the NULL terminator.
        let ptrs: [*const u8; 1] = [core::ptr::null()];
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 0);
    }

    #[test]
    fn test_count_cstring_array_one() {
        let s = b"hello\0";
        let ptrs: [*const u8; 2] = [s.as_ptr(), core::ptr::null()];
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 1);
    }

    #[test]
    fn test_count_cstring_array_three() {
        let s1 = b"one\0";
        let s2 = b"two\0";
        let s3 = b"three\0";
        let ptrs: [*const u8; 4] = [
            s1.as_ptr(), s2.as_ptr(), s3.as_ptr(), core::ptr::null(),
        ];
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 3);
    }

    // -- pack_cstring_array (existing, but add a round-trip test with count) --

    #[test]
    fn test_pack_and_count_consistency() {
        let s1 = b"alpha\0";
        let s2 = b"beta\0";
        let ptrs: [*const u8; 3] = [s1.as_ptr(), s2.as_ptr(), core::ptr::null()];

        // Count should match.
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 2);

        // Pack and verify format.
        let mut buf = [0u8; 256];
        let packed_len = pack_cstring_array(ptrs.as_ptr(), &mut buf);

        // "alpha\0beta\0" = 6 + 5 = 11 bytes.
        assert_eq!(packed_len, 11);
        assert_eq!(&buf[..6], b"alpha\0");
        assert_eq!(&buf[6..11], b"beta\0");
    }
}
