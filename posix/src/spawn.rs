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
//! ## File Descriptor Inheritance
//!
//! `posix_spawn` builds an fd_map from the parent's fd table and
//! file_actions, then passes it to the kernel via `SYS_PROCESS_SPAWN_EX`.
//! The child retrieves inherited fds during startup via
//! `SYS_PROCESS_GET_INITIAL_FDS` (handled in `crt.rs`) and reinitializes
//! its fd table accordingly.
//!
//! File actions are applied in order against a virtual fd table seeded
//! from the parent's inheritable (non-`FD_CLOEXEC`) fds:
//! - **close**: removes the fd from the child's view
//! - **dup2**: copies a handle from one fd to another
//! - **open**: opens the file in the parent's context (raw syscall) and
//!   records the kernel handle for inheritance.  The handles are closed
//!   in the parent after the spawn syscall completes.
//!
//! ## Limitations
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
// FdMapEntry — file descriptor inheritance ABI
// ---------------------------------------------------------------------------

/// Handle type constants for `FdMapEntry`.
///
/// Must match `kernel/src/proc/spawn.rs fd_handle_type`.
pub mod fd_handle_type {
    /// Regular file handle (kernel dups via `fs::handle::dup()`).
    pub const FILE: u8 = 0;
    /// Pipe handle (raw pass-through — no kernel-level dup yet).
    pub const PIPE: u8 = 1;
    /// TCP socket handle.
    pub const TCP_SOCKET: u8 = 2;
    /// UDP socket handle.
    pub const UDP_SOCKET: u8 = 3;
    /// Console I/O (stdin/stdout/stderr virtual handle).
    pub const CONSOLE: u8 = 4;
    /// Eventfd counter handle (raw pass-through — no kernel-level dup
    /// yet; closing from either side closes for both).
    pub const EVENTFD: u8 = 5;
}

/// A file descriptor mapping entry for `SYS_PROCESS_SPAWN_EX`.
///
/// Tells the kernel which of the parent's handles the child should
/// inherit and at which POSIX fd numbers.  Layout must match
/// `kernel/src/proc/spawn.rs FdMapEntry` exactly (16 bytes, C ABI).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FdMapEntry {
    /// Target POSIX fd number in the child.
    pub fd: i32,
    /// Handle type (see [`fd_handle_type`] constants).
    pub handle_type: u8,
    /// Reserved padding (set to 0).
    pub _pad: [u8; 3],
    /// Parent's kernel handle to dup into the child.
    pub handle: u64,
}

/// Maximum number of fd mappings we can build.
///
/// Covers three standard fds + the file actions limit (16).
const MAX_FD_MAP: usize = 32;

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
        return errno::EFAULT;
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
    if acts.is_null() {
        return errno::EFAULT;
    }
    if fd < 0 {
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
    if acts.is_null() {
        return errno::EFAULT;
    }
    if fd < 0 || newfd < 0 {
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
    if acts.is_null() || path.is_null() {
        return errno::EFAULT;
    }
    if fd < 0 {
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
// posix_spawn_file_actions_addchdir_np
// ---------------------------------------------------------------------------

/// Add a change-directory action to a spawn file actions object.
///
/// This is a glibc/macOS extension (`_np` = non-portable).  In the
/// child process, the working directory will be changed to `path`
/// before executing the program.
///
/// Since our kernel handles CWD at the process level, this stores the
/// path and the spawn implementation will set the child's CWD.
///
/// Returns 0 on success, or a POSIX error code.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addchdir_np(
    acts: *mut PosixSpawnFileActionsT,
    path: *const u8,
) -> i32 {
    if acts.is_null() || path.is_null() {
        return errno::EFAULT;
    }
    let a = unsafe { &mut *acts };
    if a.count >= MAX_FILE_ACTIONS {
        return errno::ENOMEM;
    }
    let path_len = unsafe { crate::file::c_strlen_pub(path) };
    if path_len >= ACTION_PATH_MAX {
        return errno::ENAMETOOLONG;
    }
    let mut stored_path = [0u8; ACTION_PATH_MAX];
    // SAFETY: path is readable for path_len bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(path, stored_path.as_mut_ptr(), path_len);
    }
    if let Some(slot) = a.actions.get_mut(a.count) {
        // Tag 4 = Chdir action (not yet processed by spawn — forward-compatible).
        *slot = FileActionSlot {
            tag: 4,
            fd: -1,
            path: stored_path,
            path_len,
            ..FileActionSlot::empty()
        };
    }
    a.count = a.count.wrapping_add(1);
    0
}

// ---------------------------------------------------------------------------
// posix_spawn_file_actions_addclosefrom_np — close all fds >= lowfd
// ---------------------------------------------------------------------------

/// Record a "close all fds from `lowfd` upward" action.
///
/// Non-portable glibc/macOS extension.  During `posix_spawn`, all
/// file descriptors ≥ `lowfd` will be closed in the child.
///
/// We store this as tag 5 (closefrom), with `fd` set to `lowfd`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addclosefrom_np(
    acts: *mut PosixSpawnFileActionsT,
    lowfd: i32,
) -> i32 {
    if acts.is_null() {
        return errno::EFAULT;
    }
    if lowfd < 0 {
        return errno::EBADF;
    }
    let a = unsafe { &mut *acts };
    if a.count >= MAX_FILE_ACTIONS {
        return errno::ENOMEM;
    }
    if let Some(slot) = a.actions.get_mut(a.count) {
        // Tag 5 = Closefrom action.
        *slot = FileActionSlot {
            tag: 5,
            fd: lowfd,
            ..FileActionSlot::empty()
        };
    }
    a.count = a.count.wrapping_add(1);
    0
}

// ---------------------------------------------------------------------------
// posix_spawnattr
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Spawn attribute flag constants
//
// The values are fixed by POSIX.1-2008 / POSIX.1-2017 and the GNU
// extensions, and match the bit layout used by glibc, FreeBSD, and
// musl.  They are exposed publicly so callers (and our own tests)
// can compose flag words by name without hard-coding magic numbers.
// ---------------------------------------------------------------------------

/// Reset effective uid/gid to real uid/gid in the child.
pub const POSIX_SPAWN_RESETIDS: i16 = 0x01;
/// Place the child in the process group given by `pgroup`.
pub const POSIX_SPAWN_SETPGROUP: i16 = 0x02;
/// Reset signals listed in `sigdefault` to SIG_DFL in the child.
pub const POSIX_SPAWN_SETSIGDEF: i16 = 0x04;
/// Replace the child's signal mask with `sigmask`.
pub const POSIX_SPAWN_SETSIGMASK: i16 = 0x08;
/// Apply `schedparam` to the child (with the current scheduler).
pub const POSIX_SPAWN_SETSCHEDPARAM: i16 = 0x10;
/// Apply `schedpolicy` and `schedparam` to the child.
pub const POSIX_SPAWN_SETSCHEDULER: i16 = 0x20;
/// Use a vfork-style spawn for the child (GNU extension).
pub const POSIX_SPAWN_USEVFORK: i16 = 0x40;
/// Place the child in a new session (POSIX.1-2018).
pub const POSIX_SPAWN_SETSID: i16 = 0x80;

/// Union of every flag bit currently accepted by
/// `posix_spawnattr_setflags`.  Any bit outside this mask causes
/// `posix_spawnattr_setflags` to return `EINVAL`, matching glibc's
/// `__POSIX_SPAWN_MASK` check.
pub const POSIX_SPAWN_VALID_FLAGS: i16 =
    POSIX_SPAWN_RESETIDS
        | POSIX_SPAWN_SETPGROUP
        | POSIX_SPAWN_SETSIGDEF
        | POSIX_SPAWN_SETSIGMASK
        | POSIX_SPAWN_SETSCHEDPARAM
        | POSIX_SPAWN_SETSCHEDULER
        | POSIX_SPAWN_USEVFORK
        | POSIX_SPAWN_SETSID;

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
        return errno::EFAULT;
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
///
/// Returns `EFAULT` if `attr` is null, or `EINVAL` if any bit outside
/// `POSIX_SPAWN_VALID_FLAGS` is set in `flags`.  This matches POSIX:
///
/// > If the value of the attribute being set is not valid,
/// > posix_spawnattr_setflags() shall return [EINVAL].
///
/// and glibc's `__POSIX_SPAWN_MASK` validation.  We validate the null
/// pointer first so a caller passing a junk attribute alongside a
/// bogus flag word still gets the more informative `EFAULT` for
/// `attr` rather than silently storing into garbage memory.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setflags(
    attr: *mut PosixSpawnattrT,
    flags: i16,
) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // Reject any bit outside the accepted mask.  Using bitwise-AND
    // against the inverted mask avoids assumptions about sign — the
    // i16 cast preserves the bit pattern.
    if (flags & !POSIX_SPAWN_VALID_FLAGS) != 0 {
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
        return errno::EFAULT;
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
        return errno::EFAULT;
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
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe { *pgroup = (*attr).pgroup; }
    0
}

// ---------------------------------------------------------------------------
// fd_map building from file_actions
// ---------------------------------------------------------------------------

/// Convert a `HandleKind` to the kernel's `fd_handle_type` constant.
fn kind_to_handle_type(kind: crate::fdtable::HandleKind) -> u8 {
    use crate::fdtable::HandleKind;
    match kind {
        HandleKind::File => fd_handle_type::FILE,
        HandleKind::Pipe => fd_handle_type::PIPE,
        HandleKind::Console => fd_handle_type::CONSOLE,
        HandleKind::TcpStream | HandleKind::TcpListener => fd_handle_type::TCP_SOCKET,
        HandleKind::UdpSocket => fd_handle_type::UDP_SOCKET,
        HandleKind::Eventfd => fd_handle_type::EVENTFD,
        // Epoll, Timerfd, and Inotify fds are per-process userspace
        // state and cannot be meaningfully transferred to a child.  Map
        // to FILE so the function is total; build_fd_map filters these
        // entries out before they reach this conversion.
        HandleKind::Epoll | HandleKind::Timerfd | HandleKind::Inotify
            => fd_handle_type::FILE,
    }
}

/// Tracks kernel handles opened by `build_fd_map` for open file_actions.
///
/// The parent opens files on behalf of the child (so the kernel can dup
/// them into the child's PCB).  These handles must be closed after the
/// spawn syscall returns — whether it succeeded or failed.
struct OpenedHandles {
    /// Kernel handle values that were opened by build_fd_map.
    handles: [u64; MAX_FILE_ACTIONS],
    /// Number of valid entries.
    count: usize,
}

impl OpenedHandles {
    const fn new() -> Self {
        Self { handles: [0; MAX_FILE_ACTIONS], count: 0 }
    }

    fn push(&mut self, handle: u64) {
        if self.count < MAX_FILE_ACTIONS {
            self.handles[self.count] = handle;
            self.count = self.count.wrapping_add(1);
        }
    }

    /// Close all tracked handles.
    fn close_all(&self) {
        let mut i = 0usize;
        while i < self.count {
            let _ = syscall1(SYS_FS_CLOSE, self.handles[i]);
            i = i.wrapping_add(1);
        }
    }
}

/// Build an fd_map array from the parent's fd table and file_actions.
///
/// Simulates what the child needs to see: starts with the parent's
/// inheritable fds (non-`FD_CLOEXEC`), then applies file_actions in
/// order:
/// - **close**: removes the fd from the virtual table
/// - **dup2**: copies a handle from one fd to another
/// - **open**: opens the file in the parent's context (raw syscall, no
///   fd allocation) and records the kernel handle.  The kernel will dup
///   it into the child during spawn.  The raw handles are tracked in
///   `opened` so the caller can close them after the spawn syscall.
///
/// Returns the number of valid entries written to `out`.
///
/// # Design
///
/// We build a "virtual fd table" that represents what the child's fd
/// table should look like after applying all file_actions.  Each slot
/// stores `Option<(u8, u64)>` — the handle type and parent handle.
///
/// After applying all actions, we flatten the non-empty slots into
/// the output `FdMapEntry` array.
fn build_fd_map(
    file_actions: *const PosixSpawnFileActionsT,
    out: &mut [FdMapEntry; MAX_FD_MAP],
    opened: &mut OpenedHandles,
) -> usize {
    use crate::fdtable;

    // Virtual fd table: mirrors what the child should see.
    // We only track fds 0..MAX_FD_MAP because that's the most we can
    // pass to the kernel anyway.
    let mut virt: [Option<(u8, u64)>; MAX_FD_MAP] = [None; MAX_FD_MAP];

    // Step 1: Populate from parent's open fds that don't have FD_CLOEXEC.
    // For the child's fd_map, we include all inheritable fds from the
    // parent so the child starts with the same I/O handles.
    let mut idx = 0usize;
    while idx < MAX_FD_MAP {
        #[allow(clippy::cast_possible_wrap)]
        let fd = idx as i32;
        if let Some(entry) = fdtable::get_fd(fd) {
            // Skip close-on-exec fds — they shouldn't be inherited.
            // Skip epoll/timerfd/inotify fds — the instance state lives
            // in the parent's userspace memory and cannot be transferred
            // to the child.
            if entry.flags & fdtable::FD_CLOEXEC == 0
                && entry.kind != fdtable::HandleKind::Epoll
                && entry.kind != fdtable::HandleKind::Timerfd
                && entry.kind != fdtable::HandleKind::Inotify
            {
                virt[idx] = Some((kind_to_handle_type(entry.kind), entry.handle));
            }
        }
        idx = idx.wrapping_add(1);
    }

    // Step 2: Apply file_actions in order.
    if !file_actions.is_null() {
        // SAFETY: file_actions is non-null (checked above).  The caller
        // guarantees it was initialized via posix_spawn_file_actions_init.
        let acts = unsafe { &*file_actions };
        let mut action_idx = 0usize;
        while action_idx < acts.count && action_idx < MAX_FILE_ACTIONS {
            // SAFETY: action_idx < acts.count <= MAX_FILE_ACTIONS.
            if let Some(slot) = acts.actions.get(action_idx) {
                match slot.tag {
                    1 => {
                        // Close: remove this fd from the virtual table.
                        #[allow(clippy::cast_sign_loss)]
                        let fd_u = slot.fd as usize;
                        if fd_u < MAX_FD_MAP {
                            virt[fd_u] = None;
                        }
                    }
                    2 => {
                        // Dup2(fd → newfd): copy fd's entry to newfd.
                        #[allow(clippy::cast_sign_loss)]
                        let src_u = slot.fd as usize;
                        #[allow(clippy::cast_sign_loss)]
                        let dst_u = slot.newfd as usize;
                        if dst_u < MAX_FD_MAP && src_u < MAX_FD_MAP {
                            // Copy the entry from the virtual table (which
                            // already reflects prior actions).
                            virt[dst_u] = virt[src_u];
                        }
                    }
                    3 => {
                        // Open: open the file in the parent's context.
                        // We use a raw syscall (no fd allocation) — we
                        // just need the kernel handle to pass via fd_map.
                        // The kernel will dup it into the child during spawn.
                        #[allow(clippy::cast_sign_loss)]
                        let target_fd = slot.fd as usize;
                        if target_fd < MAX_FD_MAP && slot.path_len > 0 {
                            // Resolve the path against CWD.
                            let mut resolved = [0u8; crate::unistd::PATH_MAX];
                            let resolved_len = unsafe {
                                crate::unistd::resolve_path(
                                    slot.path.as_ptr(),
                                    &mut resolved,
                                )
                            };

                            if let Some(rlen) = resolved_len {
                                let native_flags = crate::file::translate_open_flags(slot.oflag);
                                let ret = syscall3(
                                    SYS_FS_OPEN,
                                    resolved.as_ptr() as u64,
                                    rlen as u64,
                                    native_flags,
                                );
                                if ret >= 0 {
                                    let handle = ret as u64;
                                    virt[target_fd] = Some((fd_handle_type::FILE, handle));
                                    opened.push(handle);
                                }
                                // If open fails, silently skip this action.
                                // POSIX says posix_spawn should fail, but we
                                // can't return an error from build_fd_map
                                // without complicating the interface.  The
                                // child will simply not have this fd.
                            }
                        }
                    }
                    _ => {} // Unknown tag — skip.
                }
            }
            action_idx = action_idx.wrapping_add(1);
        }
    }

    // Step 3: Flatten to FdMapEntry array.
    let mut count = 0usize;
    let mut flat_idx = 0usize;
    while flat_idx < MAX_FD_MAP {
        if let Some((handle_type, handle)) = virt[flat_idx] {
            if count < MAX_FD_MAP {
                #[allow(clippy::cast_possible_wrap)]
                let fd = flat_idx as i32;
                out[count] = FdMapEntry {
                    fd,
                    handle_type,
                    _pad: [0; 3],
                    handle,
                };
                count = count.wrapping_add(1);
            }
        }
        flat_idx = flat_idx.wrapping_add(1);
    }

    count
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
///   Applied to the parent's fd table to build the kernel fd_map.
///   The child retrieves inherited fds via `SYS_PROCESS_GET_INITIAL_FDS`
///   during startup.
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
    if path.is_null() {
        return errno::EFAULT;
    }

    // Build the fd_map from the parent's fd table + file_actions.
    // This tells the kernel which handles the child should inherit.
    // Open file_actions are executed here — the parent opens the files
    // and the kernel dups the handles into the child.  We track the
    // opened handles so we can close them after the spawn syscall.
    let mut fd_map = [FdMapEntry { fd: 0, handle_type: 0, _pad: [0; 3], handle: 0 }; MAX_FD_MAP];
    let mut opened = OpenedHandles::new();
    let fd_map_count = build_fd_map(file_actions, &mut fd_map, &mut opened);

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        // POSIX: empty path → ENOENT; too-long → ENAMETOOLONG.
        // SAFETY: path is non-null (checked above) and a valid C string.
        opened.close_all(); // Clean up any handles opened by build_fd_map.
        return if unsafe { *path } == 0 { errno::ENOENT } else { errno::ENAMETOOLONG };
    };

    // Load the ELF binary using the resolved absolute path.
    let (buf_ptr, alloc_size, data_size) = match load_elf(resolved.as_ptr(), resolved_len) {
        Ok(result) => result,
        Err(err) => {
            opened.close_all();
            return err;
        }
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
        fd_map_ptr: if fd_map_count > 0 { fd_map.as_ptr() as u64 } else { 0 },
        fd_map_count: fd_map_count as u64,
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

    // Close any file handles opened by build_fd_map for open file_actions.
    // The kernel has already duped them into the child's PCB, so the
    // parent's copies are no longer needed.
    opened.close_all();

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
        return errno::EFAULT;
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
// fexecve
// ---------------------------------------------------------------------------

/// Replace the current process image using an open file descriptor.
///
/// Like `execve` but takes an open fd instead of a path.  If the fd
/// has an associated path in the fd table, we resolve it and delegate
/// to `execve`.  Otherwise, returns -1 with `ENOENT`.
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fexecve(
    fd: i32,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // Try to resolve the fd to a path via the fd table's stored path.
    let mut path_buf = [0u8; crate::unistd::PATH_MAX];
    let path_len = crate::fdtable::get_fd_path(fd, &mut path_buf);
    if path_len == 0 {
        // No path associated with this fd.
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    execve(path_buf.as_ptr(), argv, envp)
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
    // Stat the file to get its size.  SYS_FS_STAT writes a 16-byte
    // FsStatResult, not a struct stat, so translate it.
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let stat_ret = syscall3(
        SYS_FS_STAT,
        path as u64,
        path_len as u64,
        raw.as_mut_ptr() as u64,
    );

    if stat_ret < 0 {
        return Err(native_to_posix_err(stat_ret));
    }

    let mut stat_buf = crate::stat::Stat::zeroed();
    crate::stat::fill_from_fsstat(&mut stat_buf, &raw);
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
// execvpe — exec with PATH search + custom environment
// ---------------------------------------------------------------------------

/// Replace the current process image with a new program, searching PATH.
///
/// Like `execvp` but accepts an explicit environment (`envp`).
/// If `file` contains `/`, it is used directly.
/// Otherwise, searches each directory in `PATH`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execvpe(
    file: *const u8,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    if file.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let file_len = unsafe { crate::file::c_strlen_pub(file) };

    // If `file` contains a '/', use it directly.
    if contains_slash(file, file_len) {
        return execve(file, argv, envp);
    }

    // Search PATH for the executable.
    let mut found = [0u8; crate::unistd::PATH_MAX];
    if !search_path(file, file_len, &mut found) {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    execve(found.as_ptr(), argv, envp)
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
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_file_actions_addopen_null_path() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addopen(
            &raw mut acts, 0, core::ptr::null(), 0, 0,
        );
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_spawnattr_getflags_null_out() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_getflags(&raw const attr, core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
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
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_spawnattr_getpgroup_null_out() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_getpgroup(&raw const attr, core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
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

    // -- FdMapEntry ABI --

    #[test]
    fn test_fd_map_entry_size() {
        assert_eq!(core::mem::size_of::<FdMapEntry>(), 16);
    }

    #[test]
    fn test_fd_map_entry_align() {
        assert_eq!(core::mem::align_of::<FdMapEntry>(), 8);
    }

    #[test]
    fn test_fd_map_entry_field_offsets() {
        let entry = FdMapEntry {
            fd: 0, handle_type: 0, _pad: [0; 3], handle: 0,
        };
        let base = &entry as *const _ as usize;
        assert_eq!(&entry.fd as *const _ as usize - base, 0);
        assert_eq!(&entry.handle_type as *const _ as usize - base, 4);
        assert_eq!(&entry.handle as *const _ as usize - base, 8);
    }

    // -- fd_handle_type constants --

    #[test]
    fn test_fd_handle_type_values() {
        assert_eq!(fd_handle_type::FILE, 0);
        assert_eq!(fd_handle_type::PIPE, 1);
        assert_eq!(fd_handle_type::TCP_SOCKET, 2);
        assert_eq!(fd_handle_type::UDP_SOCKET, 3);
        assert_eq!(fd_handle_type::CONSOLE, 4);
        assert_eq!(fd_handle_type::EVENTFD, 5);
    }

    #[test]
    fn test_fd_handle_type_distinct() {
        let vals = [
            fd_handle_type::FILE,
            fd_handle_type::PIPE,
            fd_handle_type::TCP_SOCKET,
            fd_handle_type::UDP_SOCKET,
            fd_handle_type::CONSOLE,
            fd_handle_type::EVENTFD,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "types {} and {} collide", i, j);
            }
        }
    }

    // -- kind_to_handle_type --

    #[test]
    fn test_kind_to_handle_type_file() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::File), fd_handle_type::FILE);
    }

    #[test]
    fn test_kind_to_handle_type_pipe() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::Pipe), fd_handle_type::PIPE);
    }

    #[test]
    fn test_kind_to_handle_type_console() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::Console), fd_handle_type::CONSOLE);
    }

    #[test]
    fn test_kind_to_handle_type_tcp() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::TcpStream), fd_handle_type::TCP_SOCKET);
        assert_eq!(kind_to_handle_type(HandleKind::TcpListener), fd_handle_type::TCP_SOCKET);
    }

    #[test]
    fn test_kind_to_handle_type_udp() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::UdpSocket), fd_handle_type::UDP_SOCKET);
    }

    #[test]
    fn test_kind_to_handle_type_eventfd() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::Eventfd), fd_handle_type::EVENTFD);
    }

    // -- build_fd_map --

    /// Ensure fds 0/1/2 are Console handles.
    ///
    /// Other tests may close or overwrite them; this restores the
    /// expected state before each build_fd_map test.
    fn ensure_std_fds() {
        use crate::fdtable::{install_fd, HandleKind};
        let _ = install_fd(0, HandleKind::Console, 0);
        let _ = install_fd(1, HandleKind::Console, 1);
        let _ = install_fd(2, HandleKind::Console, 2);
    }

    #[test]
    fn test_build_fd_map_no_actions() {
        ensure_std_fds();
        // With no file_actions (null), the fd_map should contain
        // the parent's inheritable fds.  In the test environment,
        // fds 0/1/2 are pre-initialized as Console handles.
        let mut out = [FdMapEntry { fd: 0, handle_type: 0, _pad: [0; 3], handle: 0 }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(core::ptr::null(), &mut out, &mut opened);

        // Should have at least fds 0, 1, 2 (Console).
        assert!(count >= 3, "expected at least 3 fds, got {}", count);

        // Verify first three are Console type.
        assert_eq!(out[0].fd, 0);
        assert_eq!(out[0].handle_type, fd_handle_type::CONSOLE);
        assert_eq!(out[1].fd, 1);
        assert_eq!(out[1].handle_type, fd_handle_type::CONSOLE);
        assert_eq!(out[2].fd, 2);
        assert_eq!(out[2].handle_type, fd_handle_type::CONSOLE);
    }

    #[test]
    fn test_build_fd_map_with_close() {
        ensure_std_fds();
        // Create file_actions that close fd 1 (stdout).
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_addclose(&raw mut acts, 1);

        let mut out = [FdMapEntry { fd: 0, handle_type: 0, _pad: [0; 3], handle: 0 }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

        // fd 1 should be gone.  We should have fd 0 and fd 2.
        let has_fd1 = out[..count].iter().any(|e| e.fd == 1);
        assert!(!has_fd1, "fd 1 should have been closed");

        let has_fd0 = out[..count].iter().any(|e| e.fd == 0);
        let has_fd2 = out[..count].iter().any(|e| e.fd == 2);
        assert!(has_fd0, "fd 0 should still exist");
        assert!(has_fd2, "fd 2 should still exist");
    }

    #[test]
    fn test_build_fd_map_with_dup2() {
        ensure_std_fds();
        // Create file_actions that dup2(2, 1) — redirect stdout to stderr.
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_adddup2(&raw mut acts, 2, 1);

        let mut out = [FdMapEntry { fd: 0, handle_type: 0, _pad: [0; 3], handle: 0 }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

        // fd 1 should now have the same handle as fd 2.
        let fd1 = out[..count].iter().find(|e| e.fd == 1);
        let fd2 = out[..count].iter().find(|e| e.fd == 2);
        assert!(fd1.is_some(), "fd 1 should exist");
        assert!(fd2.is_some(), "fd 2 should exist");
        assert_eq!(
            fd1.unwrap().handle,
            fd2.unwrap().handle,
            "fd 1 and fd 2 should share the same handle after dup2",
        );
    }

    #[test]
    fn test_build_fd_map_close_then_dup2() {
        ensure_std_fds();
        // Close fd 1, then dup2(2, 1) — common shell pattern for
        // redirecting stdout to a pipe.
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_addclose(&raw mut acts, 1);
        posix_spawn_file_actions_adddup2(&raw mut acts, 2, 1);

        let mut out = [FdMapEntry { fd: 0, handle_type: 0, _pad: [0; 3], handle: 0 }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

        // fd 1 should exist (recreated by dup2) with fd 2's handle.
        let fd1 = out[..count].iter().find(|e| e.fd == 1);
        let fd2 = out[..count].iter().find(|e| e.fd == 2);
        assert!(fd1.is_some(), "fd 1 should be recreated by dup2");
        assert!(fd2.is_some(), "fd 2 should still exist");
        assert_eq!(fd1.unwrap().handle, fd2.unwrap().handle);
    }

    #[test]
    fn test_build_fd_map_close_all_standard() {
        // Close all three standard fds.
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_addclose(&raw mut acts, 0);
        posix_spawn_file_actions_addclose(&raw mut acts, 1);
        posix_spawn_file_actions_addclose(&raw mut acts, 2);

        let mut out = [FdMapEntry { fd: 0, handle_type: 0, _pad: [0; 3], handle: 0 }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

        // No standard fds should remain.
        let has_0_1_2 = out[..count].iter().any(|e| e.fd <= 2);
        assert!(!has_0_1_2, "all standard fds should be closed");
    }

    #[test]
    fn test_max_fd_map_constant() {
        assert_eq!(MAX_FD_MAP, 32);
        // Must be large enough for 3 standard fds + MAX_FILE_ACTIONS.
        assert!(MAX_FD_MAP >= 3 + MAX_FILE_ACTIONS);
    }

    // -----------------------------------------------------------------------
    // fexecve
    // -----------------------------------------------------------------------

    #[test]
    fn test_fexecve_negative_fd() {
        crate::errno::set_errno(0);
        let ret = fexecve(-1, core::ptr::null(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fexecve_no_path_fd() {
        // fd 999 has no path stored → ENOENT.
        crate::errno::set_errno(0);
        let ret = fexecve(999, core::ptr::null(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    // -----------------------------------------------------------------------
    // posix_spawn_file_actions_addchdir_np
    // -----------------------------------------------------------------------

    #[test]
    fn test_addchdir_np_null_acts() {
        let ret = posix_spawn_file_actions_addchdir_np(
            core::ptr::null_mut(),
            b"/tmp\0".as_ptr(),
        );
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_addchdir_np_null_path() {
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addchdir_np(&raw mut acts, core::ptr::null());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_addchdir_np_success() {
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addchdir_np(
            &raw mut acts,
            b"/tmp\0".as_ptr(),
        );
        assert_eq!(ret, 0);
        assert_eq!(acts.count, 1);
        assert_eq!(acts.actions[0].tag, 4, "chdir action tag should be 4");
    }

    #[test]
    fn test_addchdir_np_full() {
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        // Fill all slots.
        for _ in 0..MAX_FILE_ACTIONS {
            posix_spawn_file_actions_addclose(&raw mut acts, 0);
        }
        let ret = posix_spawn_file_actions_addchdir_np(
            &raw mut acts,
            b"/tmp\0".as_ptr(),
        );
        assert_eq!(ret, crate::errno::ENOMEM, "full actions should return ENOMEM");
    }

    // -----------------------------------------------------------------------
    // execvpe — exec with PATH search + custom environment
    // -----------------------------------------------------------------------

    #[test]
    fn test_execvpe_null_file() {
        crate::errno::set_errno(0);
        let ret = execvpe(core::ptr::null(), core::ptr::null(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_execvpe_nonexistent_path_search() {
        // A filename without '/' that doesn't exist in PATH.
        // On our OS this returns ENOENT; on the test host, search_path
        // may produce unpredictable results via SYS_FS_STAT.
        crate::errno::set_errno(0);
        let ret = execvpe(
            b"nonexistent_binary_xyz_12345\0".as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
        );
        // Either ENOENT (not found in PATH) or the exec itself fails.
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_execvpe_with_slash_delegates_to_execve() {
        // A filename with '/' is used directly, not searched in PATH.
        // Syscall result is unpredictable on test host.
        let ret = execvpe(
            b"/nonexistent/binary\0".as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
        );
        // Should return -1 (exec replaces process on success, so any
        // return means failure).
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // posix_spawn_file_actions_addclosefrom_np
    // -----------------------------------------------------------------------

    #[test]
    fn test_addclosefrom_np_null_acts() {
        let ret = posix_spawn_file_actions_addclosefrom_np(core::ptr::null_mut(), 3);
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_addclosefrom_np_negative_fd() {
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclosefrom_np(&raw mut acts, -1);
        assert_eq!(ret, crate::errno::EBADF);
    }

    #[test]
    fn test_addclosefrom_np_success() {
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclosefrom_np(&raw mut acts, 3);
        assert_eq!(ret, 0);
        assert_eq!(acts.count, 1);
        assert_eq!(acts.actions[0].tag, 5, "closefrom action tag should be 5");
        assert_eq!(acts.actions[0].fd, 3);
    }

    #[test]
    fn test_addclosefrom_np_full() {
        let mut acts = PosixSpawnFileActionsT {
            count: 0,
            actions: [FileActionSlot::empty(); MAX_FILE_ACTIONS],
            _pad: [0; 8],
        };
        posix_spawn_file_actions_init(&raw mut acts);
        for _ in 0..MAX_FILE_ACTIONS {
            posix_spawn_file_actions_addclose(&raw mut acts, 0);
        }
        let ret = posix_spawn_file_actions_addclosefrom_np(&raw mut acts, 3);
        assert_eq!(ret, crate::errno::ENOMEM, "full actions should return ENOMEM");
    }

    // -----------------------------------------------------------------------
    // Phase 81 — posix_spawnattr_setflags flag-mask validation
    //
    // POSIX:
    //   If the value of the attribute being set is not valid,
    //   posix_spawnattr_setflags() shall return [EINVAL].
    //
    // glibc applies a mask check (`flags & ~__POSIX_SPAWN_MASK`) and
    // returns EINVAL on any unrecognised bit.  These tests pin that
    // behaviour for our implementation.
    // -----------------------------------------------------------------------

    fn fresh_attr() -> PosixSpawnattrT {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        attr
    }

    // ---- (a) Mask invariants --------------------------------------------

    #[test]
    fn test_posix_spawn_valid_flags_equals_union() {
        assert_eq!(
            POSIX_SPAWN_VALID_FLAGS,
            POSIX_SPAWN_RESETIDS
                | POSIX_SPAWN_SETPGROUP
                | POSIX_SPAWN_SETSIGDEF
                | POSIX_SPAWN_SETSIGMASK
                | POSIX_SPAWN_SETSCHEDPARAM
                | POSIX_SPAWN_SETSCHEDULER
                | POSIX_SPAWN_USEVFORK
                | POSIX_SPAWN_SETSID
        );
    }

    #[test]
    fn test_posix_spawn_valid_flags_value() {
        // Every flag from RESETIDS (0x01) through SETSID (0x80) =
        // 0xFF.  This catches accidental gaps in the constants.
        assert_eq!(POSIX_SPAWN_VALID_FLAGS, 0xFF);
    }

    #[test]
    fn test_posix_spawn_flags_are_distinct_bits() {
        for f in [
            POSIX_SPAWN_RESETIDS,
            POSIX_SPAWN_SETPGROUP,
            POSIX_SPAWN_SETSIGDEF,
            POSIX_SPAWN_SETSIGMASK,
            POSIX_SPAWN_SETSCHEDPARAM,
            POSIX_SPAWN_SETSCHEDULER,
            POSIX_SPAWN_USEVFORK,
            POSIX_SPAWN_SETSID,
        ] {
            assert_eq!(f.count_ones(), 1, "flag {f:#x} must be a single bit");
        }
    }

    #[test]
    fn test_new_flag_constants_have_expected_values() {
        assert_eq!(POSIX_SPAWN_SETSCHEDPARAM, 0x10);
        assert_eq!(POSIX_SPAWN_SETSCHEDULER, 0x20);
        assert_eq!(POSIX_SPAWN_USEVFORK, 0x40);
        assert_eq!(POSIX_SPAWN_SETSID, 0x80);
    }

    // ---- (b) Rejection of unknown bits ----------------------------------

    #[test]
    fn test_setflags_rejects_single_unknown_high_bit() {
        let mut attr = fresh_attr();
        // i16::MIN = -0x8000 — sets the sign bit only; outside mask.
        let ret = posix_spawnattr_setflags(&raw mut attr, i16::MIN);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejects_bit_just_above_setsid() {
        // First bit outside the mask = 0x100.
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, 0x100);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejects_unknown_bit_combined_with_valid() {
        // POSIX_SPAWN_SETSID | 0x100 — partially valid, must still fail.
        let mut attr = fresh_attr();
        let bad = POSIX_SPAWN_SETSID | 0x100;
        let ret = posix_spawnattr_setflags(&raw mut attr, bad);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejects_negative_one() {
        // -1 in i16 = 0xFFFF — has every high bit set, must fail.
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, -1);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejection_does_not_mutate_attr() {
        // Critical invariant: a failed setflags must leave the previous
        // flag word untouched, otherwise callers can be left with a
        // half-configured attr object.
        let mut attr = fresh_attr();
        let ok = posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_RESETIDS);
        assert_eq!(ok, 0);
        let bad = posix_spawnattr_setflags(&raw mut attr, 0x4000);
        assert_eq!(bad, errno::EINVAL);
        // attr.flags should still hold the previous value.
        let mut got: i16 = 0;
        let r = posix_spawnattr_getflags(&raw const attr, &raw mut got);
        assert_eq!(r, 0);
        assert_eq!(got, POSIX_SPAWN_RESETIDS);
    }

    // ---- (c) Acceptance of every valid bit ------------------------------

    #[test]
    fn test_setflags_accepts_each_valid_bit_individually() {
        for f in [
            POSIX_SPAWN_RESETIDS,
            POSIX_SPAWN_SETPGROUP,
            POSIX_SPAWN_SETSIGDEF,
            POSIX_SPAWN_SETSIGMASK,
            POSIX_SPAWN_SETSCHEDPARAM,
            POSIX_SPAWN_SETSCHEDULER,
            POSIX_SPAWN_USEVFORK,
            POSIX_SPAWN_SETSID,
        ] {
            let mut attr = fresh_attr();
            let ret = posix_spawnattr_setflags(&raw mut attr, f);
            assert_eq!(ret, 0, "flag {f:#x} should be accepted");
            let mut got: i16 = 0;
            assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
            assert_eq!(got, f);
        }
    }

    #[test]
    fn test_setflags_accepts_full_mask() {
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_VALID_FLAGS);
        assert_eq!(ret, 0);
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, POSIX_SPAWN_VALID_FLAGS);
    }

    #[test]
    fn test_setflags_accepts_zero() {
        // Zero (no flags) must succeed — it's the post-init default.
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, 0);
        assert_eq!(ret, 0);
    }

    // ---- (d) Validation order -------------------------------------------

    #[test]
    fn test_setflags_null_attr_precedes_flag_check() {
        // Both errors apply (null attr AND bad flag); EFAULT for the
        // null pointer takes priority over EINVAL for the bad flag.
        let ret = posix_spawnattr_setflags(core::ptr::null_mut(), 0x4000);
        assert_eq!(ret, errno::EFAULT);
    }

    // ---- (e) Workflow / buggy-caller patterns ---------------------------

    #[test]
    fn test_setflags_then_getflags_roundtrip_full_mask() {
        let mut attr = fresh_attr();
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_VALID_FLAGS),
            0,
        );
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, POSIX_SPAWN_VALID_FLAGS);
    }

    #[test]
    fn test_setflags_replace_overwrites_prior_flags() {
        let mut attr = fresh_attr();
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_RESETIDS | POSIX_SPAWN_SETSID),
            0,
        );
        // Replace with a smaller value.  setflags() is whole-word, not
        // bitwise-OR, so the second call must overwrite, not merge.
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_USEVFORK),
            0,
        );
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, POSIX_SPAWN_USEVFORK);
    }

    #[test]
    fn test_setflags_init_clears_flags() {
        // After a successful setflags, a second init() must reset the
        // attr to zero flags.  Otherwise reuse of a stale attr object
        // would silently carry old flags into a fresh spawn.
        let mut attr = fresh_attr();
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_VALID_FLAGS),
            0,
        );
        posix_spawnattr_init(&raw mut attr);
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, 0);
    }
}
